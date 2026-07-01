//! The issue-list filter overlay: cycling a row's value and the Enter action
//! (person rows open a member picker; state/priority just advance).

use super::{App, FilterRow, MemberTarget};
use crate::domain::{AssigneeFilter, CreatorFilter, StateType};

impl App {
    /// Advance the value of the filter row under the cursor (`dir` is +1/-1),
    /// then refresh the list so filtering is live.
    pub(super) fn cycle_filter(&mut self, dir: i32) {
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
    pub(super) fn filter_enter(&mut self) {
        match FilterRow::ALL[self.filter_cursor] {
            FilterRow::Assignee => self.load_members_for(MemberTarget::FilterAssignee),
            FilterRow::Creator => self.load_members_for(MemberTarget::FilterCreator),
            FilterRow::State | FilterRow::Priority => self.cycle_filter(1),
        }
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
