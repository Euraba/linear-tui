//! The `/` find (highlight + `n`/`N` jump) and `f` issue-list filter: opening
//! the input bar, reacting to keystrokes, and committing/cancelling.

use super::{issue_haystack, App, EditMode, Editing, Find, FindContext, Pane};
use crate::search;

impl App {
    /// Open the `/` find bar for `context`.
    pub(super) fn start_find(&mut self, context: FindContext) {
        self.find = None;
        self.editing = Some(Editing {
            mode: EditMode::Find(context),
            buffer: String::new(),
        });
    }

    /// Open the `f` filter bar, pre-filled with the current filter for refining.
    pub(super) fn start_filter(&mut self) {
        self.editing = Some(Editing {
            mode: EditMode::Filter,
            buffer: self.filter_query.clone(),
        });
    }

    /// React to a keystroke in the input bar: the filter re-filters live; the
    /// find just re-highlights (the renderer reads the live buffer).
    pub(super) fn after_edit_change(&mut self) {
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
    pub(super) fn commit_edit(&mut self) {
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
    pub(super) fn cancel_edit(&mut self) {
        if let Some(editing) = self.editing.take() {
            match editing.mode {
                EditMode::Find(_) => self.find = None,
                EditMode::Filter => self.clear_filter(),
            }
        }
    }

    /// Remove the active `f` filter and restore the full list.
    pub(super) fn clear_filter(&mut self) {
        self.filter_query.clear();
        self.rebuild_visible();
        self.clamp_issue_selection();
        self.preview_selected_issue();
    }

    /// Move to the next (`dir > 0`) / previous (`dir < 0`) match, or the first
    /// match (`dir == 0`), for the committed find. Issues find moves the list
    /// selection; detail find scrolls between highlighted lines.
    pub(super) fn find_next(&mut self, context: FindContext, dir: i32) {
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

    /// The filter query in effect right now — the live edit buffer while the
    /// filter bar is open, otherwise the committed [`App::filter_query`].
    pub(super) fn effective_filter(&self) -> &str {
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
