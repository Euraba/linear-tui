//! Detail-pane navigation (parent / sub-issues / back-stack) and the embedded
//! image handling that feeds the image viewer.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;

use super::{App, Overlay, Pane, PickerKind};
use crate::images::{self, ImageState};
use crate::worker::Request;

impl App {
    /// The id of the issue the detail pane is showing (and the target of
    /// actions like state/assignee/comment). Normally the hovered list issue,
    /// but parent/sub-issue navigation can point it elsewhere.
    pub(super) fn current_issue_id(&self) -> Option<String> {
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
    pub(super) fn go_to_issue(&mut self, id: String) {
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
    pub(super) fn go_to_parent(&mut self) {
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
    pub(super) fn open_subissue_picker(&mut self) {
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
    pub(super) fn nav_back(&mut self) {
        if let Some(prev) = self.detail_history.pop() {
            self.navigate_to(prev);
        }
    }

    /// Re-fetch the issue currently shown, after a list reload. If the reload's
    /// preview re-pointed `detail_target` at the list selection, restore it to
    /// `id` first — so a refresh keeps a navigated parent/sub-issue on screen.
    /// Unlike [`App::navigate_to`], this preserves scroll and focus.
    pub(super) fn refresh_detail_for(&mut self, id: String) {
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
    pub(super) fn refresh_detail_images(&mut self) {
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
    pub(super) fn open_image_viewer(&mut self) {
        if self.detail_image_urls.is_empty() {
            self.status = "No images in this issue".into();
        } else {
            self.overlay = Overlay::ImageViewer { index: 0 };
        }
    }

    /// Handle a key while the image viewer overlay is open. `index` is the
    /// currently-shown image.
    pub(super) fn keys_image_viewer(&mut self, key: KeyEvent, index: usize) {
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
}
