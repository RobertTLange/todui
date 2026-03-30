use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::tui::theme::{SurfaceTone, TextTone, Theme};

pub fn render_markdown(theme: &Theme, source: &str, width: u16) -> Text<'static> {
    if width == 0 {
        return Text::default();
    }

    let mut lines = Vec::new();
    for line in source.lines() {
        lines.push(render_markdown_line(theme, line));
    }

    if source.ends_with('\n') {
        lines.push(Line::default());
    }

    Text::from(lines)
}

fn render_markdown_line(theme: &Theme, line: &str) -> Line<'static> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return Line::default();
    }

    if let Some((level, content)) = heading_content(trimmed) {
        let mut heading = Line::from(parse_inline(theme, content, heading_style(theme, level)));
        heading.style = theme.text_style(TextTone::Meta);
        return heading;
    }

    if let Some(content) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let mut spans = vec![Span::styled(
            "• ",
            theme
                .text_style(TextTone::Muted)
                .add_modifier(Modifier::BOLD),
        )];
        spans.extend(parse_inline(
            theme,
            content,
            theme.text_style(TextTone::Default),
        ));
        return Line::from(spans);
    }

    Line::from(parse_inline(
        theme,
        trimmed,
        theme.text_style(TextTone::Default),
    ))
}

fn heading_content(line: &str) -> Option<(usize, &str)> {
    let hash_count = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if hash_count == 0 || hash_count > 6 {
        return None;
    }

    let content = line[hash_count..].strip_prefix(' ')?;
    Some((hash_count, content))
}

fn heading_style(theme: &Theme, level: usize) -> Style {
    let tone = match level {
        1 | 2 => SurfaceTone::Details,
        3 | 4 => SurfaceTone::Open,
        _ => SurfaceTone::Neutral,
    };
    theme.surface_title_style(tone)
}

fn parse_inline(theme: &Theme, source: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = source;

    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style.add_modifier(Modifier::BOLD),
                ));
                remaining = &rest[end + 2..];
                continue;
            }

            spans.push(Span::styled(String::from("**"), base_style));
            remaining = rest;
            continue;
        }

        if let Some(rest) = remaining.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style.add_modifier(Modifier::ITALIC),
                ));
                remaining = &rest[end + 1..];
                continue;
            }

            spans.push(Span::styled(String::from("*"), base_style));
            remaining = rest;
            continue;
        }

        if let Some(rest) = remaining.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .patch(theme.text_style(TextTone::Meta))
                        .add_modifier(Modifier::BOLD),
                ));
                remaining = &rest[end + 1..];
                continue;
            }

            spans.push(Span::styled(String::from("`"), base_style));
            remaining = rest;
            continue;
        }

        let next_marker = next_marker_index(remaining).unwrap_or(remaining.len());
        spans.push(Span::styled(
            remaining[..next_marker].to_string(),
            base_style,
        ));
        remaining = &remaining[next_marker..];
    }

    spans
}

fn next_marker_index(source: &str) -> Option<usize> {
    ["**", "*", "`"]
        .into_iter()
        .filter_map(|marker| source.find(marker))
        .filter(|index| *index > 0)
        .min()
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::render_markdown;
    use crate::tui::theme::Theme;

    #[test]
    fn renders_headers_and_bold_text() {
        let theme = Theme::default();
        let rendered = render_markdown(&theme, "# Heading\n\nA **bold** move.", 40);

        assert_eq!(rendered.lines[0].spans[0].content.as_ref(), "Heading");
        assert!(
            rendered.lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(rendered.lines[2].spans[1].content.as_ref(), "bold");
        assert!(
            rendered.lines[2].spans[1]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }
}
