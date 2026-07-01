//! UI-side application state and the logic that turns key presses into
//! [`Request`]s. Rendering lives in `ui.rs`; this module is render-agnostic.

use std::collections::HashMap;

use ratatui::widgets::ListState;
use ratatui_image::picker::Picker;
use tokio::sync::mpsc::UnboundedSender;

use crate::cache;
use crate::domain::{
    AssigneeFilter, CreatorFilter, Filters, Issue, IssueDetail, Project, StateType, Team, User,
    View, WorkflowState,
};
use crate::images::{self, ImageState};
use crate::search;
use crate::settings::{CacheMode, Settings};
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

/// What a pending team-members fetch is for, so the [`Response::Members`]
/// handler knows which picker to open.
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
    /// Cached issue lists, keyed by [`cache::list_key`] (team+view+project).
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

    /// The filter query in effect right now — the live edit buffer while the
    /// filter bar is open, otherwise the committed [`App::filter_query`].
    fn effective_filter(&self) -> &str {
        if let Some(e) = &self.editing {
            if matches!(e.mode, EditMode::Filter) {
                return &e.buffer;
            }
        }
        &self.filter_query
    }

    /// The find query + context in effect for highlighting — the live edit
    /// buffer while a `/` bar is open, otherwise the committed [`App::find`].
    /// Returns `None` when nothing should be highlighted.
    pub fn active_find(&self) -> Option<(&str, FindContext)> {
        if let Some(e) = &self.editing {
            return match e.mode {
                EditMode::Find(ctx) => Some((e.buffer.as_str(), ctx)),
                EditMode::Filter => None,
            };
        }
        self.find.as_ref().map(|f| (f.query.as_str(), f.context))
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
            let key = cache::list_key(
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

    // ----- ticket cache (stale-while-revalidate) ------------------------

    /// Look up a cached issue detail (memory first, then disk for
    /// [`CacheMode::Disk`]), populating the memory layer from disk on a hit.
    fn cached_detail(&mut self, id: &str) -> Option<IssueDetail> {
        if !self.cache_mode.uses_memory() {
            return None;
        }
        if let Some(d) = self.detail_cache.get(id) {
            return Some(d.clone());
        }
        if self.cache_mode.uses_disk() {
            if let Some(d) = cache::read_detail(id) {
                self.detail_cache.insert(id.to_string(), d.clone());
                return Some(d);
            }
        }
        None
    }

    /// Store a freshly-fetched detail into the cache (memory, plus disk for
    /// [`CacheMode::Disk`]).
    fn store_detail(&mut self, detail: &IssueDetail) {
        if !self.cache_mode.uses_memory() {
            return;
        }
        self.detail_cache.insert(detail.id.clone(), detail.clone());
        if self.cache_mode.uses_disk() {
            cache::write_detail(detail);
        }
    }

    /// Look up a cached issue list (memory first, then disk).
    fn cached_list(&mut self, key: &str) -> Option<Vec<Issue>> {
        if !self.cache_mode.uses_memory() {
            return None;
        }
        if let Some(v) = self.issue_list_cache.get(key) {
            return Some(v.clone());
        }
        if self.cache_mode.uses_disk() {
            if let Some(v) = cache::read_list(key) {
                self.issue_list_cache.insert(key.to_string(), v.clone());
                return Some(v);
            }
        }
        None
    }

    /// Store a freshly-fetched issue list into the cache.
    fn store_list(&mut self, key: &str, issues: &[Issue]) {
        if !self.cache_mode.uses_memory() {
            return;
        }
        self.issue_list_cache
            .insert(key.to_string(), issues.to_vec());
        if self.cache_mode.uses_disk() {
            cache::write_list(key, issues);
        }
    }

    /// Cycle the cache mode (Off → Memory → Disk → Off), persist it, and drop
    /// the in-memory caches when caching is turned off.
    fn cycle_cache_mode(&mut self) {
        self.cache_mode = self.cache_mode.cycle();
        Settings {
            cache_mode: self.cache_mode,
        }
        .save();
        if !self.cache_mode.uses_memory() {
            self.detail_cache.clear();
            self.issue_list_cache.clear();
        }
        self.status = format!("Cache mode: {}", self.cache_mode.label());
    }

    // ----- search (`/` find) and filter (`f`) ---------------------------

    /// Open the `/` find bar for `context`.
    fn start_find(&mut self, context: FindContext) {
        self.find = None;
        self.editing = Some(Editing {
            mode: EditMode::Find(context),
            buffer: String::new(),
        });
    }

    /// Open the `f` filter bar, pre-filled with the current filter for refining.
    fn start_filter(&mut self) {
        self.editing = Some(Editing {
            mode: EditMode::Filter,
            buffer: self.filter_query.clone(),
        });
    }

    /// React to a keystroke in the input bar: the filter re-filters live; the
    /// find just re-highlights (the renderer reads the live buffer).
    fn after_edit_change(&mut self) {
        if matches!(
            self.editing.as_ref().map(|e| e.mode),
            Some(EditMode::Filter)
        ) {
            self.rebuild_visible();
            // fzf-style: snap to the top of the narrowed list.
            self.issues_state
                .select((!self.visible.is_empty()).then_some(0));
            self.preview_selected_issue();
        }
    }

    /// Apply the input bar (Enter): keep the filter / commit the find + jump.
    fn commit_edit(&mut self) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        match editing.mode {
            EditMode::Find(context) => {
                if editing.buffer.is_empty() {
                    self.find = None;
                    return;
                }
                self.find = Some(Find {
                    context,
                    query: editing.buffer,
                });
                self.find_detail_idx = 0;
                self.find_next(context, 0); // jump to the first match
            }
            EditMode::Filter => {
                self.filter_query = editing.buffer;
                self.rebuild_visible();
                self.clamp_issue_selection();
                self.focus = Pane::Issues;
                self.preview_selected_issue();
            }
        }
    }

    /// Cancel the input bar (Esc): a find is discarded; a filter is cleared.
    fn cancel_edit(&mut self) {
        if let Some(editing) = self.editing.take() {
            match editing.mode {
                EditMode::Find(_) => self.find = None,
                EditMode::Filter => self.clear_filter(),
            }
        }
    }

    /// Remove the active `f` filter and restore the full list.
    fn clear_filter(&mut self) {
        self.filter_query.clear();
        self.rebuild_visible();
        self.clamp_issue_selection();
        self.preview_selected_issue();
    }

    /// Move to the next (`dir > 0`) / previous (`dir < 0`) match, or the first
    /// match (`dir == 0`), for the committed find. Issues find moves the list
    /// selection; detail find scrolls between highlighted lines.
    fn find_next(&mut self, context: FindContext, dir: i32) {
        let Some(find) = &self.find else { return };
        let query = find.query.clone();
        match context {
            FindContext::Issues => {
                let matches: Vec<usize> = self
                    .visible
                    .iter()
                    .enumerate()
                    .filter(|(_, &gi)| search::is_match(&issue_haystack(&self.issues[gi]), &query))
                    .map(|(vi, _)| vi)
                    .collect();
                if matches.is_empty() {
                    self.status = format!("No match for “{query}”");
                    return;
                }
                let cur = self.issues_state.selected().unwrap_or(0);
                let next = next_match(&matches, cur, dir);
                self.issues_state.select(Some(next));
                self.preview_selected_issue();
            }
            FindContext::Detail => {
                let len = self.find_detail_lines.len();
                if len == 0 {
                    self.status = format!("No match for “{query}”");
                    return;
                }
                self.find_detail_idx = match dir {
                    0 => 0,
                    d => (self.find_detail_idx as i32 + d).rem_euclid(len as i32) as usize,
                };
                // Scroll the match near the top, leaving a little context above.
                let line = self.find_detail_lines[self.find_detail_idx] as u16;
                self.detail_scroll = line.saturating_sub(2);
            }
        }
    }

    // ----- issue navigation (parent / sub-issues) -----------------------

    /// The id of the issue the detail pane is showing (and the target of
    /// actions like state/assignee/comment). Normally the hovered list issue,
    /// but parent/sub-issue navigation can point it elsewhere.
    fn current_issue_id(&self) -> Option<String> {
        self.detail_target.clone()
    }

    /// (identifier, title) for `id`, looked up among the loaded issue list and
    /// the current detail's parent/children — used for the loading header.
    fn issue_label(&self, id: &str) -> Option<(String, String)> {
        if let Some(i) = self.issues.iter().find(|i| i.id == id) {
            return Some((i.identifier.clone(), i.title.clone()));
        }
        if let Some(d) = &self.detail {
            if let Some(p) = d.parent.as_ref().filter(|p| p.id == id) {
                return Some((p.identifier.clone(), p.title.clone()));
            }
            if let Some(c) = d.children.iter().find(|c| c.id == id) {
                return Some((c.identifier.clone(), c.title.clone()));
            }
        }
        None
    }

    /// Show `id` in the detail pane (cached instantly if available, then
    /// refreshed), syncing the list selection when the issue is on-screen.
    fn navigate_to(&mut self, id: String) {
        self.detail_stub = self.issue_label(&id);
        if let Some(vi) = self.visible.iter().position(|&gi| self.issues[gi].id == id) {
            self.issues_state.select(Some(vi));
        }
        self.detail_scroll = 0;
        self.detail_target = Some(id.clone());
        match self.cached_detail(&id) {
            Some(cached) => {
                self.detail = Some(cached);
                self.refresh_detail_images();
            }
            None => {
                self.detail = None;
                self.detail_image_urls.clear();
            }
        }
        self.request_detail(id);
        self.focus = Pane::Detail;
    }

    /// Follow a parent/sub-issue link, remembering where we came from so
    /// `Backspace` can return.
    fn go_to_issue(&mut self, id: String) {
        if self.detail_target.as_deref() == Some(id.as_str()) {
            self.focus = Pane::Detail;
            return;
        }
        if let Some(cur) = self.detail_target.clone() {
            self.detail_history.push(cur);
        }
        self.navigate_to(id);
    }

    /// Jump to the parent of the issue currently shown (if it has one).
    fn go_to_parent(&mut self) {
        let parent = self
            .detail
            .as_ref()
            .and_then(|d| d.parent.as_ref())
            .map(|p| p.id.clone());
        match parent {
            Some(id) => self.go_to_issue(id),
            None => self.status = "No parent issue".into(),
        }
    }

    /// Open a picker of the shown issue's sub-issues; choosing one navigates.
    fn open_subissue_picker(&mut self) {
        let items: Vec<(String, String)> = self
            .detail
            .as_ref()
            .map(|d| {
                d.children
                    .iter()
                    .map(|c| (c.id.clone(), format!("{} {}", c.identifier, c.title)))
                    .collect()
            })
            .unwrap_or_default();
        if items.is_empty() {
            self.status = "No sub-issues".into();
            return;
        }
        let mut state = ListState::default();
        state.select(Some(0));
        self.overlay = Overlay::Picker {
            kind: PickerKind::SubIssue,
            items,
            state,
        };
    }

    /// Go back to the previously-viewed issue (`Backspace`).
    fn nav_back(&mut self) {
        if let Some(prev) = self.detail_history.pop() {
            self.navigate_to(prev);
        }
    }

    /// Re-fetch the issue currently shown, after a list reload. If the reload's
    /// preview re-pointed `detail_target` at the list selection, restore it to
    /// `id` first — so a refresh keeps a navigated parent/sub-issue on screen.
    /// Unlike [`App::navigate_to`], this preserves scroll and focus.
    fn refresh_detail_for(&mut self, id: String) {
        if self.detail_target.as_deref() != Some(id.as_str()) {
            self.detail_stub = self.issue_label(&id);
            if let Some(vi) = self.visible.iter().position(|&gi| self.issues[gi].id == id) {
                self.issues_state.select(Some(vi));
            }
            self.detail_target = Some(id.clone());
            if let Some(cached) = self.cached_detail(&id) {
                self.detail = Some(cached);
                self.refresh_detail_images();
            }
        }
        self.request_detail(id);
    }

    /// Recompute the current issue's embedded image URLs from the loaded detail
    /// and kick off a fetch for any we haven't requested yet this session.
    fn refresh_detail_images(&mut self) {
        self.detail_image_urls = match &self.detail {
            Some(d) => images::detail_image_urls(d),
            None => Vec::new(),
        };
        for url in self.detail_image_urls.clone() {
            if !self.images.contains_key(&url) {
                self.images.insert(url.clone(), ImageState::Loading);
                self.send(Request::LoadImage { url });
            }
        }
    }

    /// Open the image viewer overlay if the current issue has any images.
    fn open_image_viewer(&mut self) {
        if self.detail_image_urls.is_empty() {
            self.status = "No images in this issue".into();
        } else {
            self.overlay = Overlay::ImageViewer { index: 0 };
        }
    }

    /// Handle a key while the image viewer overlay is open. `index` is the
    /// currently-shown image.
    fn keys_image_viewer(&mut self, key: KeyEvent, index: usize) {
        let len = self.detail_image_urls.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('v') => {
                self.overlay = Overlay::None;
            }
            KeyCode::Right
            | KeyCode::Down
            | KeyCode::Char('j')
            | KeyCode::Char('n')
            | KeyCode::Char('l')
                if len > 0 =>
            {
                self.overlay = Overlay::ImageViewer {
                    index: (index + 1) % len,
                };
            }
            KeyCode::Left
            | KeyCode::Up
            | KeyCode::Char('k')
            | KeyCode::Char('p')
            | KeyCode::Char('h')
                if len > 0 =>
            {
                self.overlay = Overlay::ImageViewer {
                    index: (index + len - 1) % len,
                };
            }
            _ => {}
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
                // Cache the result under its request's key (even if superseded
                // — we paid for the fetch).
                if let Some(key) = self.pending_list_keys.remove(&epoch) {
                    self.store_list(&key, &issues);
                }
                // Display only if it's still the current request (team/view
                // didn't switch, didn't scroll past during fast navigation).
                if epoch == self.issues_epoch {
                    self.apply_issues(issues, view);
                    self.pending_select = None;
                }
            }
            Response::IssueDetail { epoch, detail } => {
                // Cache regardless of whether it's still on screen.
                self.store_detail(&detail);
                // Ignore detail for an issue we've since scrolled away from.
                if epoch == self.detail_epoch {
                    self.detail = Some(*detail);
                    self.refresh_detail_images();
                }
            }
            Response::Image { url, image } => {
                let protocol = self.picker.new_resize_protocol(*image);
                self.images.insert(url, ImageState::Ready(protocol));
            }
            Response::ImageFailed { url, error } => {
                self.images.insert(url, ImageState::Failed(error));
            }
            Response::States(states) => self.open_state_picker(states),
            Response::Members(members) => match self.member_target {
                MemberTarget::SetAssignee => self.open_assignee_picker(members),
                MemberTarget::FilterAssignee => {
                    self.open_member_picker(members, PickerKind::FilterAssignee)
                }
                MemberTarget::FilterCreator => {
                    self.open_member_picker(members, PickerKind::FilterCreator)
                }
            },
            Response::ActionDone { message, refresh } => {
                self.status = message;
                if refresh {
                    // Reload the list, then keep the detail pane on the issue we
                    // were viewing and re-fetch it — even if that's a navigated
                    // parent/sub-issue rather than the list selection.
                    let shown = self.detail_target.clone();
                    self.reload_issues();
                    if let Some(id) = shown {
                        self.refresh_detail_for(id);
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

    /// Request the selected team's members, recording what the resulting picker
    /// is for (issue reassignment vs. an assignee/creator filter).
    fn load_members_for(&mut self, target: MemberTarget) {
        if let Some(team) = self.selected_team().cloned() {
            self.member_target = target;
            self.send(Request::LoadMembers { team_id: team.id });
        }
    }

    /// A member picker for a filter field (no "unassigned" row — use the filter
    /// overlay's cycle for that).
    fn open_member_picker(&mut self, members: Vec<User>, kind: PickerKind) {
        let items: Vec<(String, String)> = members
            .into_iter()
            .map(|m| {
                let label = m.label().to_string();
                (m.id, label)
            })
            .collect();
        let mut state = ListState::default();
        state.select((!items.is_empty()).then_some(0));
        self.overlay = Overlay::Picker { kind, items, state };
    }

    fn confirm_picker(&mut self) {
        // Take ownership of the overlay so we can drop it before sending.
        let overlay = std::mem::replace(&mut self.overlay, Overlay::None);
        let Overlay::Picker { kind, items, state } = overlay else {
            self.overlay = overlay;
            return;
        };
        let Some(idx) = state.selected() else { return };
        let Some((id, label)) = items.get(idx).cloned() else {
            return;
        };
        // Sub-issue picks navigate rather than mutate.
        if kind == PickerKind::SubIssue {
            self.go_to_issue(id);
            return;
        }
        // Filter-field picks set state and return to the filter overlay; no
        // open issue is required.
        match kind {
            PickerKind::FilterAssignee => {
                self.filters.assignee = AssigneeFilter::Person { id, label };
                self.overlay = Overlay::Filter;
                self.reload_issues();
                return;
            }
            PickerKind::FilterCreator => {
                self.filters.creator = CreatorFilter::Person { id, label };
                self.overlay = Overlay::Filter;
                self.reload_issues();
                return;
            }
            _ => {}
        }
        // State/assignee changes target the issue currently shown in the detail
        // pane (which may be a navigated parent/sub-issue, not the list row).
        let Some(issue_id) = self.current_issue_id() else {
            return;
        };
        match kind {
            PickerKind::State => self.send(Request::SetState {
                issue_id,
                state_id: id,
            }),
            PickerKind::Assignee => self.send(Request::SetAssignee {
                issue_id,
                assignee_id: (!id.is_empty()).then_some(id),
            }),
            PickerKind::SubIssue | PickerKind::FilterAssignee | PickerKind::FilterCreator => {}
        }
    }

    /// Advance the value of the filter row under the cursor (`dir` is +1/-1),
    /// then refresh the list so filtering is live.
    fn cycle_filter(&mut self, dir: i32) {
        match FilterRow::ALL[self.filter_cursor] {
            FilterRow::Assignee => {
                self.filters.assignee = cycle_assignee(&self.filters.assignee, dir)
            }
            FilterRow::Creator => self.filters.creator = cycle_creator(&self.filters.creator, dir),
            FilterRow::State => self.filters.state = cycle_state(self.filters.state, dir),
            FilterRow::Priority => {
                self.filters.priority = cycle_priority(self.filters.priority, dir)
            }
        }
        self.reload_issues();
    }

    /// Enter on a filter row: person rows open a member picker to choose a
    /// specific person; state/priority simply advance one step.
    fn filter_enter(&mut self) {
        match FilterRow::ALL[self.filter_cursor] {
            FilterRow::Assignee => self.load_members_for(MemberTarget::FilterAssignee),
            FilterRow::Creator => self.load_members_for(MemberTarget::FilterCreator),
            FilterRow::State | FilterRow::Priority => self.cycle_filter(1),
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
                if let Some(issue_id) = self.current_issue_id() {
                    self.send(Request::AddComment {
                        issue_id,
                        body: text,
                    });
                }
            }
            InputKind::CreateIssue => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::CreateIssue {
                        team_id: team.id,
                        title: text,
                        parent_id: None,
                    });
                }
            }
            // Sub-issue: same team as the `n` flow, parented to the open issue.
            InputKind::CreateSubIssue { parent_id, .. } => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::CreateIssue {
                        team_id: team.id,
                        title: text,
                        parent_id: Some(parent_id),
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
        // The search/filter input bar captures all keys while open.
        if self.editing.is_some() {
            match key.code {
                KeyCode::Esc => self.cancel_edit(),
                KeyCode::Enter => self.commit_edit(),
                KeyCode::Backspace => {
                    if let Some(e) = &mut self.editing {
                        e.buffer.pop();
                    }
                    self.after_edit_change();
                }
                KeyCode::Char(c) => {
                    if let Some(e) = &mut self.editing {
                        e.buffer.push(c);
                    }
                    self.after_edit_change();
                }
                _ => {}
            }
            return;
        }

        // The image viewer captures input; handled separately because its key
        // logic needs to read other fields of `self` (the image-url list).
        if let Overlay::ImageViewer { index } = &self.overlay {
            let index = *index;
            self.keys_image_viewer(key, index);
            return;
        }

        // With a committed `/` find, n/N jump between matches and Esc clears it.
        // (This must precede the global `n` = new-issue binding.)
        if let Some(find) = &self.find {
            let ctx = find.context;
            match key.code {
                KeyCode::Char('n') => {
                    self.find_next(ctx, 1);
                    return;
                }
                KeyCode::Char('N') => {
                    self.find_next(ctx, -1);
                    return;
                }
                KeyCode::Esc => {
                    self.find = None;
                    return;
                }
                _ => {}
            }
        }

        // Other overlays capture all input while open.
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
            Overlay::Picker { state, items, kind } => {
                let len = items.len();
                match key.code {
                    KeyCode::Esc => {
                        // Filter-field pickers return to the filter overlay so
                        // you can keep editing; other pickers just close.
                        let to_filter =
                            matches!(kind, PickerKind::FilterAssignee | PickerKind::FilterCreator);
                        self.overlay = if to_filter {
                            Overlay::Filter
                        } else {
                            Overlay::None
                        };
                    }
                    KeyCode::Enter => self.confirm_picker(),
                    KeyCode::Down | KeyCode::Char('j') => move_sel(state, len, 1),
                    KeyCode::Up | KeyCode::Char('k') => move_sel(state, len, -1),
                    _ => {}
                }
                return;
            }
            Overlay::Settings => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char(',') | KeyCode::Char('q') => {
                        self.overlay = Overlay::None
                    }
                    KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Enter
                    | KeyCode::Char('h')
                    | KeyCode::Char('l') => self.cycle_cache_mode(),
                    _ => {}
                }
                return;
            }
            Overlay::Filter => {
                let n = FilterRow::ALL.len();
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('F') => {
                        self.overlay = Overlay::None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.filter_cursor = (self.filter_cursor + 1) % n;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.filter_cursor = (self.filter_cursor + n - 1) % n;
                    }
                    KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
                        self.cycle_filter(1)
                    }
                    KeyCode::Left | KeyCode::Char('h') => self.cycle_filter(-1),
                    // Person rows open a picker; state/priority just advance.
                    KeyCode::Enter => self.filter_enter(),
                    // Clear every filter.
                    KeyCode::Char('c') => {
                        self.filters = Filters::default();
                        self.reload_issues();
                    }
                    _ => {}
                }
                return;
            }
            // Handled above (intercepted before this match) — listed for
            // exhaustiveness.
            Overlay::ImageViewer { .. } => return,
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
            KeyCode::Char(',') => {
                self.overlay = Overlay::Settings;
                return;
            }
            // Esc clears an active `f` filter (find is handled above).
            KeyCode::Esc if !self.filter_query.is_empty() => {
                self.clear_filter();
                return;
            }
            // h / l move focus left / right between panes (Tab / Shift-Tab
            // still work). j / k stay reserved for moving within a pane.
            KeyCode::Tab | KeyCode::Char('l') => {
                self.focus = self.focus.next();
                return;
            }
            KeyCode::BackTab | KeyCode::Char('h') => {
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
            // Shift-N creates a sub-issue under the currently open issue.
            KeyCode::Char('N') => {
                if self.selected_team().is_none() {
                    return;
                }
                // Clone out of the borrow before mutating `self.overlay`.
                let parent = self
                    .detail
                    .as_ref()
                    .map(|d| (d.id.clone(), d.identifier.clone()));
                match parent {
                    Some((parent_id, parent_label)) => {
                        self.overlay = Overlay::Input {
                            kind: InputKind::CreateSubIssue {
                                parent_id,
                                parent_label,
                            },
                            buffer: String::new(),
                        };
                    }
                    None => self.status = "Open an issue first to add a sub-issue (N)".into(),
                }
                return;
            }
            // Shift-F opens the issue-list filter editor (lowercase f is the
            // in-list text filter, handled per-pane).
            KeyCode::Char('F') => {
                self.filter_cursor = 0;
                self.overlay = Overlay::Filter;
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
        let len = self.visible.len();
        match key.code {
            // Hovering an issue loads its body automatically.
            KeyCode::Down | KeyCode::Char('j') => {
                move_sel(&mut self.issues_state, len, 1);
                self.preview_selected_issue();
            }
            KeyCode::Up | KeyCode::Char('k') => {
                move_sel(&mut self.issues_state, len, -1);
                self.preview_selected_issue();
            }
            // Detail is already loaded; Enter just moves focus there to scroll.
            KeyCode::Enter => self.focus = Pane::Detail,
            KeyCode::Char('/') => self.start_find(FindContext::Issues),
            KeyCode::Char('f') => self.start_filter(),
            KeyCode::Char('p') => self.go_to_parent(),
            KeyCode::Char('c') => self.open_subissue_picker(),
            KeyCode::Backspace => self.nav_back(),
            KeyCode::Char('v') => self.open_image_viewer(),
            KeyCode::Char('s') => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::LoadStates { team_id: team.id });
                }
            }
            KeyCode::Char('a') => self.load_members_for(MemberTarget::SetAssignee),
            KeyCode::Char('m') if self.detail_target.is_some() => {
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
            KeyCode::Char('/') => self.start_find(FindContext::Detail),
            KeyCode::Char('f') => self.start_filter(),
            KeyCode::Char('p') => self.go_to_parent(),
            KeyCode::Char('c') => self.open_subissue_picker(),
            KeyCode::Backspace => self.nav_back(),
            KeyCode::Char('v') => self.open_image_viewer(),
            KeyCode::Char('s') => {
                if let Some(team) = self.selected_team().cloned() {
                    self.send(Request::LoadStates { team_id: team.id });
                }
            }
            KeyCode::Char('a') => self.load_members_for(MemberTarget::SetAssignee),
            KeyCode::Char('m') if self.detail_target.is_some() => {
                self.overlay = Overlay::Input {
                    kind: InputKind::Comment,
                    buffer: String::new(),
                };
            }
            _ => {}
        }
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

/// Given sorted match positions and the current position, return the next
/// (`dir > 0`), previous (`dir < 0`), or first-at-or-after (`dir == 0`) match,
/// wrapping around.
fn next_match(matches: &[usize], cur: usize, dir: i32) -> usize {
    match dir {
        d if d > 0 => *matches.iter().find(|&&m| m > cur).unwrap_or(&matches[0]),
        d if d < 0 => *matches
            .iter()
            .rev()
            .find(|&&m| m < cur)
            .unwrap_or(matches.last().unwrap()),
        // First match at or after the cursor, else the first overall.
        _ => *matches.iter().find(|&&m| m >= cur).unwrap_or(&matches[0]),
    }
}

// ----- filter cycling ----------------------------------------------------
//
// Each cycles its field through a fixed ring of preset values. A specific
// `Person` (set via the member picker) isn't reachable by cycling — stepping
// off it lands on a neighbouring preset.

fn cycle_assignee(cur: &AssigneeFilter, dir: i32) -> AssigneeFilter {
    use AssigneeFilter::*;
    let presets = [Any, Me, Unassigned];
    let idx = match cur {
        Any => 0,
        Me => 1,
        Unassigned => 2,
        Person { .. } => {
            if dir >= 0 {
                2
            } else {
                0
            }
        }
    };
    presets[(idx + dir).rem_euclid(presets.len() as i32) as usize].clone()
}

fn cycle_creator(cur: &CreatorFilter, dir: i32) -> CreatorFilter {
    use CreatorFilter::*;
    let presets = [Any, Me];
    let idx = match cur {
        Any => 0,
        Me => 1,
        Person { .. } => {
            if dir >= 0 {
                1
            } else {
                0
            }
        }
    };
    presets[(idx + dir).rem_euclid(presets.len() as i32) as usize].clone()
}

fn cycle_state(cur: Option<StateType>, dir: i32) -> Option<StateType> {
    let all = StateType::ALL;
    // Slot 0 is "Any"; slots 1..=N are the state types.
    let idx = match cur {
        None => 0,
        Some(s) => 1 + all.iter().position(|x| *x == s).unwrap_or(0) as i32,
    };
    let next = (idx + dir).rem_euclid(all.len() as i32 + 1);
    (next != 0).then(|| all[(next - 1) as usize])
}

fn cycle_priority(cur: Option<i64>, dir: i32) -> Option<i64> {
    // Urgent, High, Medium, Low, No-priority — slot 0 is "Any".
    let vals = [1, 2, 3, 4, 0];
    let idx = match cur {
        None => 0,
        Some(p) => 1 + vals.iter().position(|v| *v == p).unwrap_or(0) as i32,
    };
    let next = (idx + dir).rem_euclid(vals.len() as i32 + 1);
    (next != 0).then(|| vals[(next - 1) as usize])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignee_cycles_through_presets() {
        use AssigneeFilter::*;
        assert_eq!(cycle_assignee(&Any, 1), Me);
        assert_eq!(cycle_assignee(&Me, 1), Unassigned);
        assert_eq!(cycle_assignee(&Unassigned, 1), Any);
        assert_eq!(cycle_assignee(&Any, -1), Unassigned);
        // A specific person isn't reachable by cycling — stepping off lands on
        // a neighbouring preset.
        let p = Person {
            id: "x".into(),
            label: "tanay".into(),
        };
        assert_eq!(cycle_assignee(&p, 1), Any);
        assert_eq!(cycle_assignee(&p, -1), Unassigned);
    }

    #[test]
    fn creator_cycle_has_no_unassigned() {
        use CreatorFilter::*;
        assert_eq!(cycle_creator(&Any, 1), Me);
        assert_eq!(cycle_creator(&Me, 1), Any);
        assert_eq!(cycle_creator(&Any, -1), Me);
    }

    #[test]
    fn state_cycle_wraps_through_any() {
        assert_eq!(cycle_state(None, 1), Some(StateType::ALL[0]));
        assert_eq!(
            cycle_state(None, -1),
            Some(StateType::ALL[StateType::ALL.len() - 1])
        );
        // A full loop of (states + Any) steps returns to Any.
        let mut s = None;
        for _ in 0..=StateType::ALL.len() {
            s = cycle_state(s, 1);
        }
        assert_eq!(s, None);
    }

    #[test]
    fn priority_cycle_includes_any_and_no_priority() {
        assert_eq!(cycle_priority(None, 1), Some(1)); // Any -> Urgent
        assert_eq!(cycle_priority(Some(0), 1), None); // No-priority -> Any
        assert_eq!(cycle_priority(None, -1), Some(0)); // Any (back) -> No-priority
    }
}
