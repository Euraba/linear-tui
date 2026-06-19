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
use ratatui_image::{Resize, StatefulImage};

use crate::app::{App, EditMode, FindContext, InputKind, Overlay, Pane, PickerKind};
use crate::images::ImageState;
use crate::models::{AssigneeFilter, CreatorFilter, Filters, View};
use crate::search;
use crate::settings::CacheMode;

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
        Overlay::None | Overlay::ImageViewer { .. } => {}
        Overlay::Help => draw_help(f),
        Overlay::Settings => draw_settings(f, app.cache_mode),
        Overlay::Filter => draw_filter(f, &app.filters, app.filter_cursor),
        Overlay::Input { kind, buffer } => draw_input(f, kind, buffer),
        Overlay::Picker { kind, items, state } => {
            draw_picker(f, *kind, items, &mut state.clone())
        }
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

fn draw_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let block = pane_block("Issue", app.focus == Pane::Detail);
    // Highlight + record `/` find matches only when the find is scoped here.
    let find_q = match app.active_find() {
        Some((q, FindContext::Detail)) if !q.is_empty() => Some(q.to_string()),
        _ => None,
    };
    let q = find_q.as_deref().unwrap_or("");

    // The detail pane shows `detail_target` — normally the hovered list issue,
    // but parent/sub-issue navigation can point it at an off-list issue.
    let target = app.detail_target.clone();
    let detail = app
        .detail
        .as_ref()
        .filter(|d| target.as_deref() == Some(d.id.as_str()));
    let image_count = app.detail_image_urls.len();

    // Rendered-line indices of detail matches, for n/N jumps.
    let mut match_lines: Vec<usize> = Vec::new();

    let lines: Vec<Line> = if let Some(d) = detail {
        // Full detail for the shown issue.
        let mut lines = Vec::new();
        push_header(
            &mut lines,
            &d.identifier,
            &d.title,
            d.state.as_ref().map(|s| s.name.as_str()),
            d.state.as_ref().and_then(|s| parse_hex_color(&s.color)),
            d.assignee.as_ref().map(|a| a.label()),
            d.priority,
            d.url.as_deref(),
        );
        // Parent link (this issue is a sub-issue).
        if let Some(p) = &d.parent {
            lines.push(Line::from(vec![
                Span::styled("↑ parent  ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", p.identifier), Style::default().fg(Color::Yellow)),
                Span::raw(p.title.clone()),
                Span::styled("   (p)", Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
        }
        if image_count > 0 {
            lines.push(Line::from(Span::styled(
                format!("🖼 {image_count} image(s) · press v to view"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        }
        if let Some(desc) = d.description.as_ref().filter(|s| !s.is_empty()) {
            for l in desc.lines() {
                let (spans, matched) = highlight_spans(l, q, Style::default());
                if matched {
                    match_lines.push(lines.len());
                }
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
        }
        // Sub-issues (children).
        if !d.children.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("── Sub-issues ({}) ──", d.children.len()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            for c in &d.children {
                let glyph = c.state.as_ref().map(|s| state_glyph(&s.kind)).unwrap_or(' ');
                let gcolor = c
                    .state
                    .as_ref()
                    .and_then(|s| parse_hex_color(&s.color))
                    .unwrap_or(ACCENT);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {glyph} "), Style::default().fg(gcolor)),
                    Span::styled(format!("{} ", c.identifier), Style::default().fg(Color::Yellow)),
                    Span::raw(c.title.clone()),
                ]));
            }
            lines.push(Line::from(Span::styled(
                "  press c to open a sub-issue",
                Style::default().fg(Color::DarkGray),
            )));
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
            let when = c.created_at.as_deref().map(short_date).unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{who} "),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(when, Style::default().fg(Color::DarkGray)),
            ]));
            for l in c.body.lines() {
                let text = format!("  {l}");
                let (spans, matched) = highlight_spans(&text, q, Style::default());
                if matched {
                    match_lines.push(lines.len());
                }
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
        }
        lines
    } else if let Some(tid) = target.as_deref() {
        // Detail still loading: instant header from the list row if the issue
        // is on-screen, otherwise from the navigation stub.
        let mut lines = Vec::new();
        if let Some(row) = app.issues.iter().find(|i| i.id == tid) {
            push_header(
                &mut lines,
                &row.identifier,
                &row.title,
                row.state.as_ref().map(|s| s.name.as_str()),
                row.state.as_ref().and_then(|s| parse_hex_color(&s.color)),
                row.assignee.as_ref().map(|a| a.label()),
                row.priority,
                None,
            );
        } else if let Some((ident, title)) = &app.detail_stub {
            push_header(&mut lines, ident, title, None, None, None, 0, None);
        } else {
            push_header(&mut lines, "…", "", None, None, None, 0, None);
        }
        lines.push(Line::from(Span::styled(
            "  Loading description & comments…",
            Style::default().fg(Color::DarkGray),
        )));
        lines
    } else {
        vec![
            Line::from(""),
            Line::from("  Select an issue — its body shows here automatically."),
            Line::from(""),
            Line::from(Span::styled(
                "  Press ? for keybindings.",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(para, area);

    // Hand the match-line indices to the app for `n`/`N` jumps (only while a
    // detail find is live; otherwise there are no detail matches to cycle).
    if find_q.is_some() {
        if app.find_detail_idx >= match_lines.len() {
            app.find_detail_idx = 0;
        }
        app.find_detail_lines = match_lines;
    } else {
        app.find_detail_lines.clear();
    }
}

/// Render the shared issue header (identifier, title, state/assignee/priority,
/// optional url) into `lines`. Used for both full detail and the loading view.
#[allow(clippy::too_many_arguments)]
fn push_header(
    lines: &mut Vec<Line<'static>>,
    identifier: &str,
    title: &str,
    state: Option<&str>,
    state_color: Option<Color>,
    assignee: Option<&str>,
    priority: i64,
    url: Option<&str>,
) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{identifier} "),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(title.to_string(), Style::default().add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("state: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            state.unwrap_or("—").to_string(),
            Style::default().fg(state_color.unwrap_or(Color::Reset)),
        ),
        Span::raw("   "),
        Span::styled("assignee: ", Style::default().fg(Color::DarkGray)),
        Span::raw(assignee.unwrap_or("Unassigned").to_string()),
        Span::raw("   "),
        Span::styled("priority: ", Style::default().fg(Color::DarkGray)),
        Span::raw(priority_label(priority).to_string()),
    ]));
    if let Some(url) = url {
        lines.push(Line::from(Span::styled(
            url.to_string(),
            Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED),
        )));
    }
    lines.push(Line::from(""));
}

/// Build a span run for `text` in `base` style, with smart-case matches of
/// `query` reverse-highlighted. Also reports whether any match was found.
fn highlight_spans(text: &str, query: &str, base: Style) -> (Vec<Span<'static>>, bool) {
    let ranges = if query.is_empty() {
        Vec::new()
    } else {
        search::match_ranges(text, query)
    };
    if ranges.is_empty() {
        return (vec![Span::styled(text.to_string(), base)], false);
    }
    let hl = Style::default()
        .bg(Color::Yellow)
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let mut spans = Vec::new();
    let mut last = 0;
    for (s, e) in ranges {
        if s > last {
            spans.push(Span::styled(text[last..s].to_string(), base));
        }
        spans.push(Span::styled(text[s..e].to_string(), hl));
        last = e;
    }
    if last < text.len() {
        spans.push(Span::styled(text[last..].to_string(), base));
    }
    (spans, true)
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    // The `/` find and `f` filter input bar takes over the status line.
    if let Some(editing) = &app.editing {
        let (sigil, action) = match editing.mode {
            EditMode::Filter => ('f', "Enter:apply  Esc:clear"),
            EditMode::Find(_) => ('/', "Enter:jump  Esc:cancel"),
        };
        let line = Line::from(vec![
            Span::styled(
                format!(" {sigil}{}\u{2588} ", editing.buffer),
                Style::default().bg(ACCENT).fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled(action, Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    // A committed `/` find shows a small indicator with jump/clear hints.
    if let Some(find) = &app.find {
        let count = match find.context {
            FindContext::Detail => format!("  {} match(es)", app.find_detail_lines.len()),
            FindContext::Issues => String::new(),
        };
        let line = Line::from(vec![
            Span::styled(
                format!(" /{}{count} ", find.query),
                Style::default().bg(ACCENT).fg(Color::White),
            ),
            Span::raw("  "),
            Span::styled(
                "n/N:jump  Esc:clear",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }

    let spinner = if app.inflight > 0 { "⣾ " } else { "" };
    let hint = match app.focus {
        Pane::Views | Pane::Teams | Pane::Projects => {
            "h/l:panes  j/k:select+load  Enter:issues  n:new  ,:settings  ?:help  q:quit"
        }
        Pane::Issues => "j/k:hover  F:filter  /:find  f:text  c:subs  v:img  s:state  a:assign  m:comment  N:new-sub  r:reload",
        Pane::Detail => "j/k:scroll  p:parent  c:subs  ⌫:back  /:find  v:img  s:state  a:assign  m:comment  N:new-sub",
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

fn draw_input(f: &mut Frame, kind: &InputKind, buffer: &str) {
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

/// Full-pane image viewer overlay. Renders the current issue's `index`-th image
/// (decoded by the worker, cached on `app`), with the title showing position.
fn draw_image_viewer(f: &mut Frame, app: &mut App, index: usize) {
    let count = app.detail_image_urls.len();
    let url = app.detail_image_urls.get(index).cloned();
    let ident = app
        .detail
        .as_ref()
        .map(|d| d.identifier.clone())
        .unwrap_or_default();

    let area = centered(f.area(), 80, 80);
    f.render_widget(Clear, area);

    let title = if count > 0 {
        format!(" {ident} · image {}/{} ", index + 1, count)
    } else {
        format!(" {ident} · images ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Span::styled(title, Style::default().fg(Color::Green)))
        .title_bottom(Line::from(" n/p: prev·next   Esc/q: close ").centered());
    let inner = block.inner(area);
    f.render_widget(block, area);

    match url.as_deref().and_then(|u| app.images.get_mut(u)) {
        Some(ImageState::Ready(protocol)) => {
            f.render_stateful_widget(StatefulImage::new().resize(Resize::Fit(None)), inner, protocol);
        }
        Some(ImageState::Failed(err)) => {
            let msg = vec![
                Line::from(Span::styled(
                    "  Could not load image",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(format!("  {err}")),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  {}", url.as_deref().unwrap_or("")),
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: false }), inner);
        }
        _ => {
            // Still loading, or the index fell out of range after a reload.
            let msg = if count == 0 { "  No images." } else { "  Loading image…" };
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray))),
                inner,
            );
        }
    }
}

/// Runtime settings panel. Currently a single row — the cache mode — with the
/// three choices listed and the active one marked.
fn draw_settings(f: &mut Frame, mode: CacheMode) {
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
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
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
        .title(Span::styled(" Settings ", Style::default().fg(Color::Green)));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_filter(f: &mut Frame, filters: &Filters, cursor: usize) {
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
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
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

fn draw_help(f: &mut Frame) {
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
