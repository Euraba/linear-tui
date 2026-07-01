//! Building and confirming the modal overlays: the state/assignee/member
//! pickers and the free-text input prompt (comment / create issue).

use ratatui::widgets::ListState;

use super::{App, InputKind, MemberTarget, Overlay, PickerKind};
use crate::domain::{AssigneeFilter, CreatorFilter, User, WorkflowState};
use crate::worker::Request;

impl App {
    pub(super) fn open_state_picker(&mut self, states: Vec<WorkflowState>) {
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

    pub(super) fn open_assignee_picker(&mut self, members: Vec<User>) {
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
    pub(super) fn load_members_for(&mut self, target: MemberTarget) {
        if let Some(team) = self.selected_team().cloned() {
            self.member_target = target;
            self.send(Request::LoadMembers { team_id: team.id });
        }
    }

    /// A member picker for a filter field (no "unassigned" row — use the filter
    /// overlay's cycle for that).
    pub(super) fn open_member_picker(&mut self, members: Vec<User>, kind: PickerKind) {
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

    pub(super) fn confirm_picker(&mut self) {
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

    pub(super) fn confirm_input(&mut self) {
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
