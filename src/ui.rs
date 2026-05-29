//! All ratatui rendering. Layout mirrors slack-tui: a narrow left column of
//! stacked list boxes (Teams over Views), a middle issue list, and a wide
//! detail pane, with a status line along the bottom. Overlays draw on top.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Wrap,
    },
    Frame,
};

use crate::app::{App, InputKind, Overlay, Pane, PickerKind};
use crate::models::View;

const ACCENT: Color = Color::Rgb(94, 106, 210); // Linear's purple-ish accent.

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
        Overlay::None => {}
        Overlay::Help => draw_help(f),
        Overlay::Input { kind, buffer } => draw_input(f, *kind, buffer),
        Overlay::Picker { kind, items, state } => {
            draw_picker(f, *kind, items, &mut state.clone())
        }
    }
}

/// A block whose border lights up when its pane has focus.
fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let border_style = if focused {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title_style = if focused {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(format!(" {title} "), title_style))
}

fn selected_style() -> Style {
    Style::default().bg(ACCENT).fg(Color::White).add_modifier(Modifier::BOLD)
}

fn draw_teams(f: &mut Frame, app: &mut App, area: Rect) {
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

fn draw_views(f: &mut Frame, app: &mut App, area: Rect) {
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

fn draw_projects(f: &mut Frame, app: &mut App, area: Rect) {
    let mut items: Vec<ListItem> =
        vec![ListItem::new(Span::styled("— All projects —", Style::default().fg(Color::Gray)))];
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

fn draw_issues(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = app
        .issues
        .iter()
        .map(|i| {
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
            ListItem::new(Line::from(vec![
                Span::styled(format!("{state} "), Style::default().fg(state_color)),
                Span::styled(format!("{prio:<4} "), priority_style(i.priority)),
                Span::styled(
                    format!("{} ", i.identifier),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(title),
                Span::styled(
                    format!("  {assignee}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let title = format!("Issues · {}", app.current_view.label());
    let list = List::new(items)
        .block(pane_block(&title, app.focus == Pane::Issues))
        .highlight_style(selected_style())
        .highlight_symbol("▌");
    f.render_stateful_widget(list, area, &mut app.issues_state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let block = pane_block("Issue", app.focus == Pane::Detail);
    let text: Text = match &app.detail {
        None => Text::from(vec![
            Line::from(""),
            Line::from("  Select an issue and press Enter to open it."),
            Line::from(""),
            Line::from(Span::styled(
                "  Press ? for keybindings.",
                Style::default().fg(Color::DarkGray),
            )),
        ]),
        Some(d) => {
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", d.identifier),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(d.title.clone(), Style::default().add_modifier(Modifier::BOLD)),
            ]));
            let state = d.state.as_ref().map(|s| s.name.as_str()).unwrap_or("—");
            let state_color = d
                .state
                .as_ref()
                .and_then(|s| parse_hex_color(&s.color))
                .unwrap_or(Color::Reset);
            let assignee = d
                .assignee
                .as_ref()
                .map(|a| a.label())
                .unwrap_or("Unassigned");
            lines.push(Line::from(vec![
                Span::styled("state: ", Style::default().fg(Color::DarkGray)),
                Span::styled(state.to_string(), Style::default().fg(state_color)),
                Span::raw("   "),
                Span::styled("assignee: ", Style::default().fg(Color::DarkGray)),
                Span::raw(assignee.to_string()),
                Span::raw("   "),
                Span::styled("priority: ", Style::default().fg(Color::DarkGray)),
                Span::raw(priority_label(d.priority).to_string()),
            ]));
            if let Some(url) = &d.url {
                lines.push(Line::from(Span::styled(
                    url.clone(),
                    Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
                )));
            }
            lines.push(Line::from(""));

            if let Some(desc) = d.description.as_ref().filter(|s| !s.is_empty()) {
                for l in desc.lines() {
                    lines.push(Line::from(l.to_string()));
                }
                lines.push(Line::from(""));
            }

            lines.push(Line::from(Span::styled(
                format!("── Comments ({}) ──", d.comments.len()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            if d.comments.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (none)",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            for c in &d.comments {
                let who = c.user.as_ref().map(|u| u.label()).unwrap_or("someone");
                let when = c
                    .created_at
                    .as_deref()
                    .map(short_date)
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{who} "),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(when, Style::default().fg(Color::DarkGray)),
                ]));
                for l in c.body.lines() {
                    lines.push(Line::from(format!("  {l}")));
                }
                lines.push(Line::from(""));
            }
            Text::from(lines)
        }
    };

    let para = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(para, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let spinner = if app.inflight > 0 { "⣾ " } else { "" };
    let hint = match app.focus {
        Pane::Views | Pane::Teams | Pane::Projects => {
            "j/k:select+load  Enter:focus issues  n:new  ?:help  q:quit"
        }
        Pane::Issues => "Enter:open  s:state  a:assign  m:comment  n:new  r:reload  ?:help",
        Pane::Detail => "j/k:scroll  s:state  a:assign  m:comment  Tab:next  ?:help",
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {spinner}{} ", app.status),
            Style::default().bg(ACCENT).fg(Color::White),
        ),
        Span::raw("  "),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ----- overlays ---------------------------------------------------------

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
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

fn draw_input(f: &mut Frame, kind: InputKind, buffer: &str) {
    let area = centered(f.area(), 60, 20);
    f.render_widget(Clear, area);
    let title = match kind {
        InputKind::Comment => " New comment (Enter to send, Esc to cancel) ",
        InputKind::CreateIssue => " New issue title (Enter to create, Esc to cancel) ",
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

fn draw_picker(
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

fn draw_help(f: &mut Frame) {
    let area = centered(f.area(), 60, 70);
    f.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "linear-tui — keybindings",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Tab / Shift+Tab   cycle pane focus"),
        Line::from("  j / k  ↑ / ↓       move selection / scroll"),
        Line::from("  (views, teams & projects load issues as you select)"),
        Line::from("  Enter             open issue / focus issue list"),
        Line::from(""),
        Line::from("  s                 change issue state"),
        Line::from("  a                 change assignee"),
        Line::from("  m                 add a comment"),
        Line::from("  n                 create a new issue"),
        Line::from("  r                 reload current issue list"),
        Line::from(""),
        Line::from("  ?                 toggle this help"),
        Line::from("  q / Ctrl-C        quit"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let para = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(" Help "),
        );
    f.render_widget(para, area.inner(Margin::new(0, 0)));
}

// ----- small helpers -----------------------------------------------------

fn priority_style(p: i64) -> Style {
    let c = match p {
        1 => Color::Red,
        2 => Color::LightRed,
        3 => Color::Yellow,
        4 => Color::Blue,
        _ => Color::DarkGray,
    };
    Style::default().fg(c)
}

/// Parse a Linear `#rrggbb` color into a ratatui [`Color`], if present.
fn parse_hex_color(hex: &Option<String>) -> Option<Color> {
    let h = hex.as_ref()?.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn state_glyph(kind: &str) -> char {
    match kind {
        "completed" => '✓',
        "canceled" => '✗',
        "started" => '◐',
        "unstarted" => '○',
        "backlog" => '·',
        "triage" => '△',
        _ => '•',
    }
}

fn priority_label(p: i64) -> &'static str {
    match p {
        1 => "Urgent",
        2 => "High",
        3 => "Medium",
        4 => "Low",
        _ => "None",
    }
}

/// Trim an ISO-8601 timestamp down to `YYYY-MM-DD HH:MM`.
fn short_date(s: &str) -> String {
    s.replace('T', " ")
        .chars()
        .take(16)
        .collect::<String>()
}
