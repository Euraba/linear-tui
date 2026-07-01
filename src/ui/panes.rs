//! The list panes: Teams, Views, Projects, and the middle Issues list.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
    Frame,
};

use super::markdown::highlight_spans;
use super::style::{pane_block, parse_hex_color, priority_style, selected_style, ACCENT};
use crate::app::{App, FindContext, Pane};
use crate::domain::{state_glyph, View};

pub(super) fn draw_teams(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .teams
        .iter()
        .map(|t| ListItem::new(format!("{}  {}", t.key, t.name)))
        .collect();
    let list = List::new(items)
        .block(pane_block("Teams", app.focus == Pane::Teams))
        .highlight_style(selected_style());
    f.render_stateful_widget(list, area, &mut app.teams_state);
}

pub(super) fn draw_views(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = View::ALL
        .iter()
        .map(|v| {
            let marker = if *v == app.current_view { "● " } else { "  " };
            ListItem::new(format!("{marker}{}", v.label()))
        })
        .collect();
    let list = List::new(items)
        .block(pane_block("Views", app.focus == Pane::Views))
        .highlight_style(selected_style());
    f.render_stateful_widget(list, area, &mut app.views_state);
}

pub(super) fn draw_projects(f: &mut Frame, app: &mut App, area: Rect) {
    let mut items: Vec<ListItem> = vec![ListItem::new(Span::styled(
        "— All projects —",
        Style::default().fg(Color::Gray),
    ))];
    items.extend(app.projects.iter().map(|p| {
        let glyph = match p.state.as_deref() {
            Some("completed") => "✓ ",
            Some("canceled") => "✗ ",
            Some("paused") => "‖ ",
            _ => "▸ ",
        };
        ListItem::new(format!("{glyph}{}", p.name))
    }));
    let list = List::new(items)
        .block(pane_block("Projects", app.focus == Pane::Projects))
        .highlight_style(selected_style());
    f.render_stateful_widget(list, area, &mut app.projects_state);
}

pub(super) fn draw_issues(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize;
    // Highlight `/` find matches in the title (only when the find is scoped to
    // the issue list).
    let find_q = match app.active_find() {
        Some((q, FindContext::Issues)) => q.to_string(),
        _ => String::new(),
    };
    let items: Vec<ListItem> = app
        .visible
        .iter()
        .map(|&gi| {
            let i = &app.issues[gi];
            let state = i
                .state
                .as_ref()
                .map(|s| state_glyph(&s.kind))
                .unwrap_or(' ');
            let state_color = i
                .state
                .as_ref()
                .and_then(|s| parse_hex_color(&s.color))
                .unwrap_or(ACCENT);
            let assignee = i
                .assignee
                .as_ref()
                .map(|a| a.label().chars().take(8).collect::<String>())
                .unwrap_or_else(|| "—".into());
            let prio = i.priority_label();
            // glyph + priority + identifier + title, with assignee trailing.
            let head_len = 2 + prio.len() + 1 + i.identifier.len() + 1;
            let avail = inner_width.saturating_sub(head_len + assignee.len() + 2);
            let title: String = i.title.chars().take(avail.max(4)).collect();
            let mut spans = vec![
                Span::styled(format!("{state} "), Style::default().fg(state_color)),
                Span::styled(format!("{prio:<4} "), priority_style(i.priority)),
                Span::styled(
                    format!("{} ", i.identifier),
                    Style::default().fg(Color::Yellow),
                ),
            ];
            spans.extend(highlight_spans(&title, &find_q, Style::default()).0);
            spans.push(Span::styled(
                format!("  {assignee}"),
                Style::default().fg(Color::DarkGray),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();

    // "Issues · <view> [· <filters>] [(visible/total)]".
    let mut title = format!("Issues · {}", app.current_view.label());
    if app.filters.is_active() {
        title.push_str(" · ");
        title.push_str(&app.filters.summary());
    }
    // Show "(visible/total)" when an `f` text filter is hiding rows.
    if app.visible.len() != app.issues.len() {
        title.push_str(&format!(" ({}/{})", app.visible.len(), app.issues.len()));
    }
    let list = List::new(items)
        .block(pane_block(&title, app.focus == Pane::Issues))
        .highlight_style(selected_style())
        .highlight_symbol("▌");
    f.render_stateful_widget(list, area, &mut app.issues_state);
}
