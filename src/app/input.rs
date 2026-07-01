//! Key dispatch: the master `on_key` handler (overlay/find capture, global
//! keys) and the per-pane movement handlers.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use super::{App, FilterRow, FindContext, InputKind, Overlay, Pane, PickerKind};
use crate::domain::{Filters, View};
use crate::worker::Request;

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
            KeyCode::Char('a') => self.load_members_for(super::MemberTarget::SetAssignee),
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
            KeyCode::Char('a') => self.load_members_for(super::MemberTarget::SetAssignee),
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
