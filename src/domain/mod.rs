//! The Linear domain model and provider-agnostic display formatting.
//!
//! Everything here is free of frontend concerns (no ratatui, no HTTP): the TUI,
//! the one-shot CLI, and the `serve` JSON-RPC mode all build on these types.
//! Submodule contents are re-exported flatly so callers use `crate::domain::X`
//! regardless of which file `X` lives in.

pub mod filters;
pub mod format;
pub mod models;
pub mod view;

pub use filters::{AssigneeFilter, CreatorFilter, Filters, StateType};
pub use format::{priority_label, short_date, state_glyph};
pub use models::{Issue, IssueDetail, Project, Team, User, WorkflowState};
pub use view::View;
