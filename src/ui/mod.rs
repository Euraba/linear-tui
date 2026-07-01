//! All ratatui rendering. Layout mirrors slack-tui: a narrow left column of
//! stacked list boxes (Teams over Views), a middle issue list, and a wide
//! detail pane, with a status line along the bottom. Overlays draw on top.
//!
//! [`draw`] owns the layout and delegates to the per-area modules: [`panes`]
//! (the four lists), [`detail`] (the issue pane), [`status`] (the bottom bar),
//! [`overlays`] (input/picker/settings/filter/help) and [`image`] (the image
//! viewer). Shared styling lives in [`style`] and markdown rendering in
//! [`markdown`].

mod detail;
mod image;
mod markdown;
mod overlays;
mod panes;
mod status;
mod style;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

use crate::app::{App, Overlay};

use detail::draw_detail;
use image::draw_image_viewer;
use overlays::{draw_filter, draw_help, draw_input, draw_picker, draw_settings};
use panes::{draw_issues, draw_projects, draw_teams, draw_views};
use status::draw_status;

pub fn draw(f: &mut Frame, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(38),
            Constraint::Percentage(40),
        ])
        .split(root[0]);

    // Left column top-to-bottom: Views (fixed, just 4 rows), a compact Teams
    // pane, then Projects filling the rest.
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ])
        .split(cols[0]);

    draw_views(f, app, left[0]);
    draw_teams(f, app, left[1]);
    draw_projects(f, app, left[2]);
    draw_issues(f, app, cols[1]);
    draw_detail(f, app, cols[2]);
    draw_status(f, app, root[1]);

    match &app.overlay {
        Overlay::None | Overlay::ImageViewer { .. } => {}
        Overlay::Help => draw_help(f),
        Overlay::Settings => draw_settings(f, app.cache_mode),
        Overlay::Filter => draw_filter(f, &app.filters, app.filter_cursor),
        Overlay::Input { kind, buffer } => draw_input(f, kind, buffer),
        Overlay::Picker { kind, items, state } => draw_picker(f, *kind, items, &mut state.clone()),
    }

    // The image viewer needs `&mut App` (rendering mutates the cached protocol),
    // so it's handled outside the immutable match above. Extract the index with
    // a short-lived borrow first to free `app` for the mutable call.
    let viewer_index = match &app.overlay {
        Overlay::ImageViewer { index } => Some(*index),
        _ => None,
    };
    if let Some(index) = viewer_index {
        draw_image_viewer(f, app, index);
    }
}
