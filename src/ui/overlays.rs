//! The modal overlays drawn on top of the main layout: input prompt, picker,
//! image-less settings/filter/help panels, plus the `centered` layout helper
//! shared with the image viewer.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::style::{selected_style, ACCENT};
use crate::app::{InputKind, PickerKind};
use crate::domain::{priority_label, AssigneeFilter, CreatorFilter, Filters};
use crate::settings::CacheMode;

/// Center a rectangle occupying `pct_x`% × `pct_y`% of `area`.
pub(super) fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}

pub(super) fn draw_input(f: &mut Frame, kind: &InputKind, buffer: &str) {
    let area = centered(f.area(), 60, 20);
    f.render_widget(Clear, area);
    let title = match kind {
        InputKind::Comment => " New comment (Enter to send, Esc to cancel) ".to_string(),
        InputKind::CreateIssue => " New issue title (Enter to create, Esc to cancel) ".to_string(),
        InputKind::CreateSubIssue { parent_label, .. } => {
            format!(" New sub-issue of {parent_label} (Enter to create, Esc to cancel) ")
        }
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(title, Style::default().fg(Color::Green)));
    let para = Paragraph::new(format!("{buffer}▌"))
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

pub(super) fn draw_picker(
    f: &mut Frame,
    kind: PickerKind,
    items: &[(String, String)],
    state: &mut ratatui::widgets::ListState,
) {
    let area = centered(f.area(), 50, 60);
    f.render_widget(Clear, area);
    let title = match kind {
        PickerKind::State => " Set state (Enter to apply, Esc to cancel) ",
        PickerKind::Assignee => " Set assignee (Enter to apply, Esc to cancel) ",
        PickerKind::SubIssue => " Open sub-issue (Enter to open, Esc to cancel) ",
        PickerKind::FilterAssignee => " Filter by assignee (Enter to pick, Esc to go back) ",
        PickerKind::FilterCreator => " Filter by creator (Enter to pick, Esc to go back) ",
    };
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|(_, label)| ListItem::new(label.clone()))
        .collect();
    let list = List::new(list_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(Span::styled(title, Style::default().fg(Color::Green))),
        )
        .highlight_style(selected_style())
        .highlight_symbol("▌");
    f.render_stateful_widget(list, area, state);
}

/// Runtime settings panel. Currently a single row — the cache mode — with the
/// three choices listed and the active one marked.
pub(super) fn draw_settings(f: &mut Frame, mode: CacheMode) {
    let area = centered(f.area(), 56, 50);
    f.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(Span::styled(
            "linear-tui — settings",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Cache mode   ", Style::default().fg(Color::Gray)),
            Span::styled(format!(" ‹ {} › ", mode.label()), selected_style()),
        ]),
        Line::from(""),
    ];
    for m in CacheMode::ALL {
        let current = m == mode;
        let style = if current {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} {:<7}", if current { "●" } else { " " }, m.label()),
                style,
            ),
            Span::styled(
                format!("  {}", m.describe()),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ←/→ or Enter: change    Esc: close (saved)",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(
            " Settings ",
            Style::default().fg(Color::Green),
        ));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

pub(super) fn draw_filter(f: &mut Frame, filters: &Filters, cursor: usize) {
    let area = centered(f.area(), 58, 45);
    f.render_widget(Clear, area);

    // Current value of each row, in FilterRow::ALL order.
    let assignee = match &filters.assignee {
        AssigneeFilter::Any => "(any)".to_string(),
        AssigneeFilter::Me => "me".to_string(),
        AssigneeFilter::Unassigned => "unassigned".to_string(),
        AssigneeFilter::Person { label, .. } => label.clone(),
    };
    let creator = match &filters.creator {
        CreatorFilter::Any => "(any)".to_string(),
        CreatorFilter::Me => "me".to_string(),
        CreatorFilter::Person { label, .. } => label.clone(),
    };
    let state = filters
        .state
        .map(|s| s.label().to_string())
        .unwrap_or_else(|| "(any)".into());
    let priority = filters
        .priority
        .map(priority_label)
        .map(str::to_string)
        .unwrap_or_else(|| "(any)".into());
    let rows = [
        ("Assignee", assignee),
        ("Creator", creator),
        ("State", state),
        ("Priority", priority),
    ];

    let mut lines = vec![
        Line::from(Span::styled(
            "linear-tui — filter issues",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (i, (label, value)) in rows.iter().enumerate() {
        let selected = i == cursor;
        let marker = if selected { "▌" } else { " " };
        let label_style = if selected {
            selected_style()
        } else {
            Style::default().fg(Color::Gray)
        };
        let value_style = if selected {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} {label:<10}"), label_style),
            Span::styled(format!(" ‹ {value} ›"), value_style),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  j/k: row   h/l: change   Enter: pick a person (assignee/creator)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(Span::styled(
        "  c: clear all    Esc / F: close    (filters apply live)",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(" Filter ", Style::default().fg(Color::Green)));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

pub(super) fn draw_help(f: &mut Frame) {
    let area = centered(f.area(), 60, 70);
    f.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "linear-tui — keybindings",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  h / l             move focus left / right between panes"),
        Line::from("  Tab / Shift+Tab   same (cycle pane focus)"),
        Line::from("  j / k  ↑ / ↓       move selection / scroll within a pane"),
        Line::from("  (selecting anything loads automatically — no Enter)"),
        Line::from("  Enter             focus issue list / detail pane"),
        Line::from(""),
        Line::from("  /                 find — jump to matches (n/N to cycle)"),
        Line::from("  f                 filter the issue list to matches (text)"),
        Line::from("  F                 filter issues by assignee/creator/state/priority"),
        Line::from("  p                 go to parent issue"),
        Line::from("  c                 open a sub-issue   (⌫ to go back)"),
        Line::from("  v                 view embedded images (n/p to cycle)"),
        Line::from("  s                 change issue state"),
        Line::from("  a                 change assignee"),
        Line::from("  m                 add a comment"),
        Line::from("  n                 create a new issue"),
        Line::from("  N                 create a sub-issue under the open issue"),
        Line::from("  r                 reload current issue list"),
        Line::from(""),
        Line::from("  ,                 settings (cache mode)"),
        Line::from("  ?                 toggle this help"),
        Line::from("  q / Ctrl-C        quit"),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", crate::sponsor::one_liner()),
            Style::default().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  Press any key to close.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let para = Paragraph::new(lines).alignment(Alignment::Left).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(" Help "),
    );
    f.render_widget(para, area.inner(Margin::new(0, 0)));
}
