//! The right-hand detail pane: header, description (markdown), sub-issues, and
//! comments — plus the shared header used by the loading placeholder.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

use super::markdown::push_body_lines;
use super::style::{pane_block, parse_hex_color, ACCENT};
use crate::app::{App, FindContext, Pane};
use crate::domain::{priority_label, short_date, state_glyph};

pub(super) fn draw_detail(f: &mut Frame, app: &mut App, area: Rect) {
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
                Span::styled(
                    format!("{} ", p.identifier),
                    Style::default().fg(Color::Yellow),
                ),
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
            push_body_lines(&mut lines, &mut match_lines, desc, q, "");
            lines.push(Line::from(""));
        }
        // Sub-issues (children).
        if !d.children.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("── Sub-issues ({}) ──", d.children.len()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            for c in &d.children {
                let glyph = c
                    .state
                    .as_ref()
                    .map(|s| state_glyph(&s.kind))
                    .unwrap_or(' ');
                let gcolor = c
                    .state
                    .as_ref()
                    .and_then(|s| parse_hex_color(&s.color))
                    .unwrap_or(ACCENT);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {glyph} "), Style::default().fg(gcolor)),
                    Span::styled(
                        format!("{} ", c.identifier),
                        Style::default().fg(Color::Yellow),
                    ),
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
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(when, Style::default().fg(Color::DarkGray)),
            ]));
            push_body_lines(&mut lines, &mut match_lines, &c.body, q, "  ");
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
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            title.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
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
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED),
        )));
    }
    lines.push(Line::from(""));
}
