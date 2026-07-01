//! Shared ratatui styling primitives: the accent colour, pane block/selection
//! styles, and Linear-specific colour helpers.

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders},
};

/// Linear's purple-ish accent, used for selection bars and section headings.
pub(super) const ACCENT: Color = Color::Rgb(94, 106, 210);

/// A block whose border lights up when its pane has focus.
pub(super) fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let border_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let title_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(format!(" {title} "), title_style))
}

pub(super) fn selected_style() -> Style {
    Style::default()
        .bg(ACCENT)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

pub(super) fn priority_style(p: i64) -> Style {
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
pub(super) fn parse_hex_color(hex: &Option<String>) -> Option<Color> {
    let h = hex.as_ref()?.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
