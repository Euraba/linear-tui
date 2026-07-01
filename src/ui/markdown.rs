//! Rendering an issue's markdown body into ratatui spans: search highlighting,
//! fenced code blocks, and inline markdown (`code`, **bold**, links).

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::style::ACCENT;
use crate::search;

/// Build a span run for `text` in `base` style, with smart-case matches of
/// `query` reverse-highlighted. Also reports whether any match was found.
pub(super) fn highlight_spans(text: &str, query: &str, base: Style) -> (Vec<Span<'static>>, bool) {
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

/// Soft green used for both fenced and inline code so code reads as code across
/// light and dark terminals without relying on a background fill.
const CODE_FG: Color = Color::Rgb(152, 195, 121);

/// Render a markdown body into styled lines. Fenced code blocks (```` ``` ````)
/// get a dim bordered gutter and monospace-green text; inline `` `code` `` spans
/// are coloured too. Everything is still search-highlighted. `indent` is
/// prepended to every emitted line (comments indent their bodies by two spaces),
/// and any line that contains a search match records its rendered index into
/// `match_lines` for `n`/`N` jumps.
pub(super) fn push_body_lines(
    lines: &mut Vec<Line<'static>>,
    match_lines: &mut Vec<usize>,
    body: &str,
    query: &str,
    indent: &str,
) {
    let gutter = Style::default().fg(Color::DarkGray);
    let code_base = Style::default().fg(CODE_FG);
    let mut in_code = false;
    for raw in body.lines() {
        if let Some(rest) = raw.trim_start().strip_prefix("```") {
            // A fence line toggles the code block; render it as a dim border,
            // showing the language tag (if any) when the block opens.
            in_code = !in_code;
            let border = if in_code {
                let lang = rest.trim();
                if lang.is_empty() {
                    format!("{indent}┌─")
                } else {
                    format!("{indent}┌─ {lang}")
                }
            } else {
                format!("{indent}└─")
            };
            lines.push(Line::from(Span::styled(border, gutter)));
            continue;
        }
        if in_code {
            // Literal code line: dim gutter + green text, no inline markdown.
            let mut spans = vec![Span::styled(format!("{indent}│ "), gutter)];
            let (code, matched) = highlight_spans(raw, query, code_base);
            if matched {
                match_lines.push(lines.len());
            }
            spans.extend(code);
            lines.push(Line::from(spans));
            continue;
        }
        // ATX heading (`#`..`######` + space): render bold + accent, dropping
        // the markers. Inline markdown inside a heading is left literal.
        let trimmed = raw.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            let title = trimmed[hashes + 1..].trim_start();
            let text = format!("{indent}{title}");
            let style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
            let (spans, matched) = highlight_spans(&text, query, style);
            if matched {
                match_lines.push(lines.len());
            }
            lines.push(Line::from(spans));
            continue;
        }
        // Prose line: inline markdown (code / bold / links), search-highlighted.
        let text = format!("{indent}{raw}");
        let (spans, matched) = inline_md_spans(&text, query);
        if matched {
            match_lines.push(lines.len());
        }
        lines.push(Line::from(spans));
    }
}

/// Soft blue for `[text](url)` link labels.
const LINK_FG: Color = Color::Rgb(97, 175, 239);

/// Render a prose line as spans, applying inline markdown — `` `code` ``,
/// `**bold**`, and `[text](url)` links — then search-highlighting within each
/// styled segment so matches still reverse-video. Returns the spans and whether
/// any match was found.
fn inline_md_spans(text: &str, query: &str) -> (Vec<Span<'static>>, bool) {
    let mut spans = Vec::new();
    let mut matched = false;
    for (content, style) in parse_inline(text) {
        let (s, m) = highlight_spans(&content, query, style);
        spans.extend(s);
        matched |= m;
    }
    (spans, matched)
}

/// Tokenise a line into `(text, style)` runs for inline markdown. Markers are
/// matched non-greedily on the same line; an unterminated marker is emitted as
/// literal text. `![alt](url)` image links render their alt text dimmed (the
/// image itself is surfaced separately in the detail header).
fn parse_inline(text: &str) -> Vec<(String, Style)> {
    let plain = Style::default();
    let code = Style::default().fg(CODE_FG);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let link = Style::default()
        .fg(LINK_FG)
        .add_modifier(Modifier::UNDERLINED);
    let alt = Style::default().fg(Color::DarkGray);

    let mut out: Vec<(String, Style)> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        // Inline code: `code` (takes priority so markers inside stay literal).
        if let Some(stripped) = rest.strip_prefix('`') {
            if let Some(close) = stripped.find('`') {
                flush(&mut out, &mut buf, plain);
                out.push((stripped[..close].to_string(), code));
                i += 1 + close + 1;
                continue;
            }
        }
        // Link: [text](url) — or an image ![alt](url) when preceded by `!`.
        if rest.starts_with('[') {
            if let Some((label, len)) = parse_link(rest) {
                let is_image = buf.ends_with('!');
                if is_image {
                    buf.pop();
                }
                flush(&mut out, &mut buf, plain);
                out.push((label, if is_image { alt } else { link }));
                i += len;
                continue;
            }
        }
        // Bold: **text**.
        if let Some(stripped) = rest.strip_prefix("**") {
            if let Some(close) = stripped.find("**") {
                flush(&mut out, &mut buf, plain);
                out.push((stripped[..close].to_string(), bold));
                i += 2 + close + 2;
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut out, &mut buf, plain);
    out
}

/// Flush the pending plain-text buffer into `out` as a styled run.
fn flush(out: &mut Vec<(String, Style)>, buf: &mut String, style: Style) {
    if !buf.is_empty() {
        out.push((std::mem::take(buf), style));
    }
}

/// Parse a `[label](url)` link at the start of `rest`, returning the label and
/// the number of bytes consumed (through the closing `)`). `None` if the shape
/// doesn't match.
fn parse_link(rest: &str) -> Option<(String, usize)> {
    let close_br = rest.find(']')?;
    let after = &rest[close_br + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let close_par = after.find(')')?;
    let label = rest[1..close_br].to_string();
    Some((label, close_br + 1 + close_par + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Concatenate a line's span contents back into a string.
    fn text_of(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn fenced_block_gets_borders_and_gutters() {
        let mut lines = Vec::new();
        let mut matches = Vec::new();
        let body = "before\n```rust\nfn main() {}\n```\nafter";
        push_body_lines(&mut lines, &mut matches, body, "", "");

        let rendered: Vec<String> = lines.iter().map(text_of).collect();
        assert_eq!(
            rendered,
            vec!["before", "┌─ rust", "│ fn main() {}", "└─", "after"]
        );
        // Code text is styled distinctly from prose.
        let code_line = &lines[2];
        assert!(code_line.spans.iter().any(|s| s.style.fg == Some(CODE_FG)));
        assert!(matches.is_empty());
    }

    #[test]
    fn inline_code_is_styled_and_searchable() {
        let (spans, _) = inline_md_spans("call `foo()` now", "");
        let code = spans
            .iter()
            .find(|s| s.content.as_ref() == "foo()")
            .expect("inline code segment");
        assert_eq!(code.style.fg, Some(CODE_FG));

        // A search match inside a code block is reported for n/N jumps.
        let mut lines = Vec::new();
        let mut matches = Vec::new();
        push_body_lines(&mut lines, &mut matches, "```\nneedle\n```", "needle", "");
        assert_eq!(matches, vec![1]);
    }

    #[test]
    fn unterminated_backtick_is_plain_text() {
        let (spans, _) = inline_md_spans("a `dangling tick", "");
        assert_eq!(
            spans.iter().map(|s| s.content.as_ref()).collect::<String>(),
            "a `dangling tick"
        );
        assert!(spans.iter().all(|s| s.style.fg != Some(CODE_FG)));
    }

    #[test]
    fn bold_and_links_are_styled() {
        let (spans, _) = inline_md_spans("see **this** and [docs](https://x.io) ok", "");
        let bold = spans
            .iter()
            .find(|s| s.content.as_ref() == "this")
            .expect("bold segment");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));

        // Link shows only the label, styled + underlined; the URL is hidden.
        let link = spans
            .iter()
            .find(|s| s.content.as_ref() == "docs")
            .expect("link label");
        assert_eq!(link.style.fg, Some(LINK_FG));
        assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));
        let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "see this and docs ok");
    }

    #[test]
    fn image_link_drops_bang_and_dims_alt() {
        let (spans, _) = inline_md_spans("![diagram](https://x.io/a.png)", "");
        let rendered: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "diagram");
        assert!(spans.iter().any(|s| s.style.fg == Some(Color::DarkGray)));
    }

    #[test]
    fn adversarial_inputs_do_not_panic() {
        // Multibyte/emoji around every marker, empty and unterminated markers,
        // and shapes that only look like links — none of these must panic on
        // the byte-index slicing in parse_inline / parse_link / heading detect.
        let cases = [
            "café `naïve()` ☕",
            "## Café ☕ heading",
            "###### é",
            "####### not a heading é",
            "**bøld** and *italic* café",
            "empty code `` and empty bold **** ok",
            "[café](http://x.io/é) ☕",
            "![diagram é](http://x.io/a.png)",
            "unterminated `code and [link](and **bold",
            "array[0](x) café",
            "```rust é\nfn café() {}\n```\ntrailing é",
            "`",
            "**",
            "[",
            "[]()",
            "☕☕☕",
            "",
        ];
        for body in cases {
            let mut lines = Vec::new();
            let mut matches = Vec::new();
            // Exercise both indents and a multibyte search query.
            push_body_lines(&mut lines, &mut matches, body, "café", "");
            push_body_lines(&mut lines, &mut matches, body, "é", "  ");
        }
    }

    #[test]
    fn headings_are_bold_accent_without_markers() {
        let mut lines = Vec::new();
        let mut matches = Vec::new();
        push_body_lines(&mut lines, &mut matches, "## Overview\nbody", "", "");
        assert_eq!(text_of(&lines[0]), "Overview");
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
        assert_eq!(text_of(&lines[1]), "body");
    }
}
