//! Handling of [`Response`] messages coming back from the background worker.

use super::{App, MemberTarget, PickerKind};
use crate::images::ImageState;
use crate::worker::Response;

impl App {
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
}
