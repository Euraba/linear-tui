//! The full-pane image viewer overlay.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use ratatui_image::{Resize, StatefulImage};

use super::overlays::centered;
use crate::app::App;
use crate::images::ImageState;

/// Full-pane image viewer overlay. Renders the current issue's `index`-th image
/// (decoded by the worker, cached on `app`), with the title showing position.
pub(super) fn draw_image_viewer(f: &mut Frame, app: &mut App, index: usize) {
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
            f.render_stateful_widget(
                StatefulImage::new().resize(Resize::Fit(None)),
                inner,
                protocol,
            );
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
            let msg = if count == 0 {
                "  No images."
            } else {
                "  Loading image…"
            };
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(Color::DarkGray))),
                inner,
            );
        }
    }
}
