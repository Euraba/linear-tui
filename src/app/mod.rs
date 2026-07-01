//! UI-side application state and the logic that turns key presses into
//! [`Request`]s. Rendering lives in `ui`; this module is render-agnostic.
//!
//! `App` is one big state machine. Its methods are split across sibling files
//! by concern — [`input`] (key dispatch), [`find`] (`/` search + `f` filter),
//! [`filter`] (the filter overlay), [`detail`] (parent/sub-issue navigation and
//! image handling), [`overlay`] (pickers + input prompts), [`response`] (worker
//! messages), and [`cache`] (the stale-while-revalidate ticket cache) — all as
//! further `impl App` blocks on the struct defined here.

mod cache;
mod detail;
mod filter;
mod find;
mod input;
mod overlay;
mod response;

use std::collections::HashMap;

use ratatui::widgets::ListState;
use ratatui_image::picker::Picker;
use tokio::sync::mpsc::UnboundedSender;

use crate::domain::{Filters, Issue, IssueDetail, Project, Team, User, View};
use crate::images::ImageState;
use crate::search;
use crate::settings::CacheMode;
use crate::worker::Request;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    Comment,
    CreateIssue,
    /// Create a sub-issue under an already-open issue. Carries the parent's id
    /// (for the mutation) and identifier (for the prompt label).
    CreateSubIssue {
        parent_id: String,
        parent_label: String,
    },
}

/// A selectable-list overlay (state picker / assignee picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    State,
    Assignee,
    /// Choose a sub-issue to open (navigates rather than mutating).
    SubIssue,
    /// Pick a person for the assignee filter (sets state, doesn't mutate).
    FilterAssignee,
    /// Pick a person for the creator filter.
    FilterCreator,
}

/// What a pending team-members fetch is for, so the `Response::Members` handler
/// knows which picker to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberTarget {
    /// Reassign the open issue (the `a` key).
    SetAssignee,
    /// Set the assignee filter.
    FilterAssignee,
    /// Set the creator filter.
    FilterCreator,
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
    /// Full-pane image viewer for the current issue's embedded images.
    /// `index` points into [`App::detail_image_urls`].
    ImageViewer {
        index: usize,
    },
    /// Runtime settings panel (currently the cache mode).
    Settings,
    /// Issue-list filter editor (assignee / creator / state / priority).
    Filter,
    Help,
}

/// Rows of the [`Overlay::Filter`] editor, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterRow {
    Assignee,
    Creator,
    State,
    Priority,
}

impl FilterRow {
    pub const ALL: [FilterRow; 4] = [
        FilterRow::Assignee,
        FilterRow::Creator,
        FilterRow::State,
        FilterRow::Priority,
    ];
}

/// Which context a `/` find is scoped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindContext {
    /// Jump the selection between matching rows in the issue list.
    Issues,
    /// Highlight + scroll between matches in the open issue's text.
    Detail,
}

/// What the bottom input bar is currently editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// `/` find query for the given context.
    Find(FindContext),
    /// `f` issue-list filter query.
    Filter,
}

/// The live text-input bar (search/filter), shown along the status line. While
/// it's open it captures all key input.
pub struct Editing {
    pub mode: EditMode,
    pub buffer: String,
}

/// A committed `/` find. Matches are highlighted; `n`/`N` cycle through them.
pub struct Find {
    pub context: FindContext,
    pub query: String,
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

    /// Extra issue-list filters (assignee/creator/state/priority), layered on
    /// top of the selected view + project. Edited in [`Overlay::Filter`].
    pub filters: Filters,
    /// Selected row in the filter overlay.
    pub filter_cursor: usize,
    /// What the in-flight `LoadMembers` will populate a picker for.
    pub member_target: MemberTarget,

    pub issues: Vec<Issue>,
    pub issues_state: ListState,
    /// Indices into `issues` that are currently shown — i.e. the list after the
    /// active `f` filter. `issues_state` selects within *this* list. With no
    /// filter it's simply `0..issues.len()`.
    pub visible: Vec<usize>,
    pub current_view: View,
    /// Bumped on every issue (re)load so stale responses can be dropped.
    issues_epoch: u64,
    /// Issue id to re-select after the next list reload (preserves your place
    /// across `r`/mutation refreshes; falls back to the top otherwise).
    pending_select: Option<String>,
    /// Maps an in-flight issue-list request's epoch to its cache key, so the
    /// response can be stored under the right key even if it's been superseded.
    pending_list_keys: HashMap<u64, String>,
    /// Cached issue lists, keyed by [`list_key`](crate::cache::list_key)
    /// (team+view+project).
    issue_list_cache: HashMap<String, Vec<Issue>>,

    pub detail: Option<IssueDetail>,
    pub detail_scroll: u16,
    /// Bumped on every detail load so a slow fetch for a no-longer-hovered
    /// issue can't overwrite the current one.
    detail_epoch: u64,
    /// Issue id the detail pane is showing. Normally the hovered list issue,
    /// but parent/sub-issue navigation can point it at an off-list issue.
    pub detail_target: Option<String>,
    /// (identifier, title) of `detail_target` for the loading header when the
    /// target isn't in the current list (e.g. a navigated parent/sub-issue).
    pub detail_stub: Option<(String, String)>,
    /// Back-stack of previously-viewed issue ids for `Backspace` navigation.
    detail_history: Vec<String>,
    /// Image URLs embedded in the currently-loaded issue, in display order.
    /// Drives the detail-pane hint and the image viewer.
    pub detail_image_urls: Vec<String>,

    /// Cached issue details, keyed by issue id (the in-memory layer; with
    /// [`CacheMode::Disk`] there's also a disk layer behind it).
    detail_cache: HashMap<String, IssueDetail>,

    /// Terminal graphics protocol + font size, used to build image protocols.
    pub picker: Picker,
    /// Session-long cache of decoded images, keyed by URL. Survives navigating
    /// between issues so revisiting one doesn't re-decode.
    pub images: HashMap<String, ImageState>,

    /// Current ticket cache mode, modifiable in the Settings overlay.
    pub cache_mode: CacheMode,

    /// Active search/filter input bar (`/` or `f`), if open.
    pub editing: Option<Editing>,
    /// Committed `/` find (highlight + jump), if any.
    pub find: Option<Find>,
    /// Active `f` filter query ("" = no filter). See [`App::visible`].
    pub filter_query: String,
    /// Line indices of detail-pane matches, recomputed by the renderer each
    /// frame; `n`/`N` scroll between them.
    pub find_detail_lines: Vec<usize>,
    /// Which entry of `find_detail_lines` is the current detail match.
    pub find_detail_idx: usize,

    pub focus: Pane,
    pub overlay: Overlay,
    pub status: String,
    pub inflight: u32,
    pub should_quit: bool,
}

impl App {
    pub fn new(tx: UnboundedSender<Request>, picker: Picker, cache_mode: CacheMode) -> Self {
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
            filters: Filters::default(),
            filter_cursor: 0,
            member_target: MemberTarget::SetAssignee,
            issues: Vec::new(),
            issues_state: ListState::default(),
            visible: Vec::new(),
            current_view: View::ALL[0],
            issues_epoch: 0,
            pending_select: None,
            pending_list_keys: HashMap::new(),
            issue_list_cache: HashMap::new(),
            detail: None,
            detail_scroll: 0,
            detail_epoch: 0,
            detail_target: None,
            detail_stub: None,
            detail_history: Vec::new(),
            detail_image_urls: Vec::new(),
            detail_cache: HashMap::new(),
            picker,
            images: HashMap::new(),
            cache_mode,
            editing: None,
            find: None,
            filter_query: String::new(),
            find_detail_lines: Vec::new(),
            find_detail_idx: 0,
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
        // `issues_state` selects within the visible (post-filter) list, so map
        // the selected position back to an index into `issues`.
        let global = *self.visible.get(self.issues_state.selected()?)?;
        self.issues.get(global)
    }

    /// Rebuild [`App::visible`] from the active `f` filter. Call after the issue
    /// list or the filter query changes.
    fn rebuild_visible(&mut self) {
        let filter = self.effective_filter().to_string();
        self.visible = if filter.is_empty() {
            (0..self.issues.len()).collect()
        } else {
            (0..self.issues.len())
                .filter(|&i| search::is_match(&issue_haystack(&self.issues[i]), &filter))
                .collect()
        };
    }

    /// Keep `issues_state` within the visible list (e.g. after it shrank).
    fn clamp_issue_selection(&mut self) {
        let len = self.visible.len();
        if len == 0 {
            self.issues_state.select(None);
        } else {
            let cur = self.issues_state.selected().unwrap_or(0).min(len - 1);
            self.issues_state.select(Some(cur));
        }
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
            let key = crate::cache::list_key(
                &team.id,
                view,
                project_id.as_deref(),
                &self.filters.signature(),
            );
            // Remember the current issue so we can re-select it once the new
            // list arrives (no-op if it's not in the new list).
            self.pending_select = self.selected_issue().map(|i| i.id.clone());
            self.issues_epoch += 1;
            self.pending_list_keys
                .insert(self.issues_epoch, key.clone());
            self.status = format!("Loading {} · {}…", team.key, view.label());
            // Stale-while-revalidate: show the cached list instantly if we have
            // one, then refresh over the network below.
            match self.cached_list(&key) {
                Some(cached) => self.apply_issues(cached, view),
                None => {
                    self.issues.clear();
                    self.rebuild_visible();
                    self.issues_state.select(None);
                }
            }
            self.send(Request::LoadIssues {
                team_id: team.id,
                view,
                project_id,
                filters: self.filters.clone(),
                epoch: self.issues_epoch,
            });
        }
    }

    /// Apply a freshly-loaded (or cached) issue list: install it, restore a
    /// sensible selection, update the status line, and preview the selection.
    fn apply_issues(&mut self, issues: Vec<Issue>, view: View) {
        // Keep the issue the user is currently looking at if it survived the
        // reload; else the one remembered at reload time; else the top.
        let keep = self
            .selected_issue()
            .map(|i| i.id.clone())
            .or_else(|| self.pending_select.clone());
        self.issues = issues;
        self.rebuild_visible();
        // Selection is an index into the visible list, so resolve `keep` there.
        let idx = keep
            .and_then(|id| self.visible.iter().position(|&i| self.issues[i].id == id))
            .or_else(|| (!self.visible.is_empty()).then_some(0));
        self.issues_state.select(idx);
        self.status = format!("{} issue(s) · {}", self.issues.len(), view.label());
        self.preview_selected_issue();
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

    /// Fire a detail fetch for `issue_id`, bumping the epoch so older fetches
    /// are ignored when they land.
    fn request_detail(&mut self, issue_id: String) {
        self.detail_epoch += 1;
        self.send(Request::LoadIssueDetail {
            issue_id,
            epoch: self.detail_epoch,
        });
    }

    /// Load the detail of the currently-hovered issue, unless it's already
    /// loaded/loading. Called whenever the issue selection changes.
    fn preview_selected_issue(&mut self) {
        let Some(issue) = self.selected_issue().cloned() else {
            self.detail = None;
            self.detail_target = None;
            self.detail_image_urls.clear();
            return;
        };
        if self.detail_target.as_deref() == Some(issue.id.as_str()) {
            return; // already showing this issue
        }
        // Moving the list selection is a fresh navigation root.
        self.detail_history.clear();
        self.detail_target = Some(issue.id.clone());
        self.detail_stub = Some((issue.identifier.clone(), issue.title.clone()));
        self.detail_scroll = 0;
        // Stale-while-revalidate: show a cached detail instantly if we have it
        // (and load its images); otherwise drop the previous issue's image list
        // so `v` can't open a viewer onto the wrong issue's images.
        match self.cached_detail(&issue.id) {
            Some(cached) => {
                self.detail = Some(cached);
                self.refresh_detail_images();
            }
            None => self.detail_image_urls.clear(),
        }
        // Always refresh over the network (even on a cache hit).
        self.request_detail(issue.id);
    }
}

/// The searchable text of an issue row (identifier, title, assignee, state,
/// priority) used by both `/` find and `f` filter.
fn issue_haystack(i: &Issue) -> String {
    let assignee = i.assignee.as_ref().map(|a| a.label()).unwrap_or("");
    let state = i.state.as_ref().map(|s| s.name.as_str()).unwrap_or("");
    format!(
        "{} {} {} {} {}",
        i.identifier,
        i.title,
        assignee,
        state,
        i.priority_label()
    )
}
