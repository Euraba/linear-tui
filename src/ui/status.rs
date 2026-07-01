//! The bottom status line: the `/`find and `f`filter input bar, the committed
//! find indicator, or the default status + per-pane key hints.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::style::ACCENT;
use crate::app::{App, EditMode, FindContext, Pane};

pub(super) fn draw_status(f: &mut Frame, app: &App, area: Rect) {
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
            Span::styled("n/N:jump  Esc:clear", Style::default().fg(Color::DarkGray)),
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
