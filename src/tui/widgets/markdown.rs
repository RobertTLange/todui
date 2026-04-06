use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::tui::theme::{TextTone, Theme};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextLink {
    pub line_index: u16,
    pub start: u16,
    pub width: u16,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkHitbox {
    pub area: Rect,
    pub url: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RenderedTextBlock {
    pub text: Text<'static>,
    pub links: Vec<TextLink>,
}

#[derive(Debug, Clone)]
struct InlineFragment {
    content: String,
    style: Style,
    url: Option<String>,
}

#[derive(Debug, Clone)]
struct StyledCell {
    symbol: char,
    style: Style,
    url: Option<String>,
}

pub(crate) fn render_markdown(theme: &Theme, source: &str, width: u16) -> RenderedTextBlock {
    if width == 0 {
        return RenderedTextBlock::default();
    }

    let mut lines = Vec::new();
    let mut links = Vec::new();
    for line in source.lines() {
        push_rendered_line(
            &mut lines,
            &mut links,
            render_markdown_line(theme, line, width),
        );
    }

    if source.ends_with('\n') {
        lines.push(Line::default());
    }

    RenderedTextBlock {
        text: Text::from(lines),
        links,
    }
}

pub(crate) fn render_labeled_text(
    theme: &Theme,
    label: &str,
    value: &str,
    width: u16,
) -> RenderedTextBlock {
    if width == 0 {
        return RenderedTextBlock::default();
    }

    let prefix = format!("{label}: ");
    let continuation_prefix = " ".repeat(prefix.chars().count());
    let logical_lines = if value.is_empty() {
        vec![""]
    } else {
        value.split('\n').collect::<Vec<_>>()
    };

    let mut lines = Vec::new();
    let mut links = Vec::new();
    for (index, logical_line) in logical_lines.into_iter().enumerate() {
        let rendered = render_prefixed_line(
            parse_inline_fragments(
                theme,
                logical_line,
                theme.text_style(TextTone::Default),
                false,
            ),
            if index == 0 {
                &prefix
            } else {
                &continuation_prefix
            },
            &continuation_prefix,
            width,
            theme.text_style(TextTone::Default),
        );
        push_rendered_line(&mut lines, &mut links, rendered);
    }

    RenderedTextBlock {
        text: Text::from(lines),
        links,
    }
}

pub(crate) fn link_hitboxes(area: Rect, links: &[TextLink]) -> Vec<LinkHitbox> {
    if area.width <= 2 || area.height <= 2 {
        return Vec::new();
    }

    let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
    links
        .iter()
        .filter_map(|link| {
            if link.line_index >= inner.height || link.start >= inner.width {
                return None;
            }

            let width = link.width.min(inner.width.saturating_sub(link.start));
            if width == 0 {
                return None;
            }

            Some(LinkHitbox {
                area: Rect::new(inner.x + link.start, inner.y + link.line_index, width, 1),
                url: link.url.clone(),
            })
        })
        .collect()
}

fn render_markdown_line(theme: &Theme, line: &str, width: u16) -> RenderedTextBlock {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return RenderedTextBlock {
            text: Text::from(vec![Line::default()]),
            links: Vec::new(),
        };
    }

    if let Some((level, content)) = heading_content(trimmed) {
        return render_wrapped_fragments(
            parse_inline_fragments(theme, content, heading_style(theme, level), true),
            width,
        );
    }

    if let Some(content) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        return render_prefixed_line(
            parse_inline_fragments(theme, content, theme.text_style(TextTone::Default), true),
            "• ",
            "  ",
            width,
            theme
                .text_style(TextTone::Muted)
                .add_modifier(Modifier::BOLD),
        );
    }

    render_wrapped_fragments(
        parse_inline_fragments(theme, trimmed, theme.text_style(TextTone::Default), true),
        width,
    )
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
        1 | 2 => TextTone::Focus,
        3 | 4 => TextTone::Tag,
        _ => TextTone::Open,
    };
    theme.text_style(tone).add_modifier(Modifier::BOLD)
}

fn parse_inline_fragments(
    theme: &Theme,
    source: &str,
    base_style: Style,
    markdown_enabled: bool,
) -> Vec<InlineFragment> {
    let mut spans = Vec::new();
    let mut remaining = source;

    while !remaining.is_empty() {
        if markdown_enabled && let Some(rest) = remaining.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                spans.extend(parse_inline_fragments(
                    theme,
                    &rest[..end],
                    base_style.add_modifier(Modifier::BOLD),
                    markdown_enabled,
                ));
                remaining = &rest[end + 2..];
                continue;
            }

            spans.push(InlineFragment {
                content: String::from("**"),
                style: base_style,
                url: None,
            });
            remaining = rest;
            continue;
        }

        if markdown_enabled && let Some(rest) = remaining.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                spans.extend(parse_inline_fragments(
                    theme,
                    &rest[..end],
                    base_style.add_modifier(Modifier::ITALIC),
                    markdown_enabled,
                ));
                remaining = &rest[end + 1..];
                continue;
            }

            spans.push(InlineFragment {
                content: String::from("*"),
                style: base_style,
                url: None,
            });
            remaining = rest;
            continue;
        }

        if markdown_enabled && let Some(rest) = remaining.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                spans.push(InlineFragment {
                    content: rest[..end].to_string(),
                    style: base_style
                        .patch(theme.text_style(TextTone::Meta))
                        .add_modifier(Modifier::BOLD),
                    url: None,
                });
                remaining = &rest[end + 1..];
                continue;
            }

            spans.push(InlineFragment {
                content: String::from("`"),
                style: base_style,
                url: None,
            });
            remaining = rest;
            continue;
        }

        let next_marker = markdown_enabled
            .then(|| next_marker_index(remaining))
            .flatten();
        let next_url = next_url_index(remaining);
        let next_event = min_index(next_marker, next_url).unwrap_or(remaining.len());

        if next_event > 0 {
            spans.push(InlineFragment {
                content: remaining[..next_event].to_string(),
                style: base_style,
                url: None,
            });
            remaining = &remaining[next_event..];
            continue;
        }

        if let Some(url_match) = parse_url_prefix(remaining) {
            spans.push(InlineFragment {
                content: url_match.visible.to_string(),
                style: link_style(theme, base_style),
                url: Some(url_match.visible.to_string()),
            });
            if !url_match.trailing.is_empty() {
                spans.push(InlineFragment {
                    content: url_match.trailing.to_string(),
                    style: base_style,
                    url: None,
                });
            }
            remaining = &remaining[url_match.consumed..];
            continue;
        }

        spans.push(InlineFragment {
            content: remaining.chars().next().unwrap_or_default().to_string(),
            style: base_style,
            url: None,
        });
        remaining = &remaining[remaining.chars().next().unwrap_or_default().len_utf8()..];
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

fn next_url_index(source: &str) -> Option<usize> {
    let mut candidates = Vec::new();
    for marker in ["https://", "http://"] {
        let mut offset = 0;
        while let Some(found) = source[offset..].find(marker) {
            let index = offset + found;
            let previous = source[..index].chars().last();
            if previous.is_none_or(is_url_boundary) {
                candidates.push(index);
                break;
            }
            offset = index.saturating_add(marker.len());
        }
    }

    candidates.into_iter().min()
}

fn is_url_boundary(character: char) -> bool {
    character.is_whitespace() || !character.is_alphanumeric()
}

fn min_index(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn link_style(theme: &Theme, base_style: Style) -> Style {
    base_style
        .patch(theme.text_style(TextTone::Open))
        .add_modifier(Modifier::UNDERLINED)
}

fn render_prefixed_line(
    fragments: Vec<InlineFragment>,
    prefix: &str,
    continuation_prefix: &str,
    width: u16,
    prefix_style: Style,
) -> RenderedTextBlock {
    let content_cells = fragments_to_cells(&fragments);
    let wrapped = wrap_content_cells(
        &content_cells,
        width.saturating_sub(prefix.chars().count() as u16),
        width.saturating_sub(continuation_prefix.chars().count() as u16),
    );
    let mut final_lines = Vec::new();
    for (index, content_line) in wrapped.into_iter().enumerate() {
        let mut line_cells = prefix_cells(
            if index == 0 {
                prefix
            } else {
                continuation_prefix
            },
            prefix_style,
        );
        line_cells.extend(content_line);
        final_lines.push(line_cells);
    }
    build_rendered_block(final_lines)
}

fn render_wrapped_fragments(fragments: Vec<InlineFragment>, width: u16) -> RenderedTextBlock {
    build_rendered_block(wrap_content_cells(
        &fragments_to_cells(&fragments),
        width,
        width,
    ))
}

fn push_rendered_line(
    lines: &mut Vec<Line<'static>>,
    links: &mut Vec<TextLink>,
    rendered: RenderedTextBlock,
) {
    let line_offset = lines.len() as u16;
    lines.extend(rendered.text.lines);
    links.extend(rendered.links.into_iter().map(|mut link| {
        link.line_index += line_offset;
        link
    }));
}

fn build_rendered_block(lines: Vec<Vec<StyledCell>>) -> RenderedTextBlock {
    let mut rendered_lines = Vec::new();
    let mut links = Vec::new();

    for (line_index, cells) in lines.into_iter().enumerate() {
        let mut spans = Vec::new();
        let mut current_content = String::new();
        let mut current_style = None;
        let mut current_url: Option<String> = None;
        let mut cursor = 0u16;
        let mut link_start = 0u16;
        let mut link_width = 0u16;

        for cell in cells {
            let width = cell_width(cell.symbol);
            if current_style == Some(cell.style) && current_url == cell.url {
                current_content.push(cell.symbol);
            } else {
                if !current_content.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut current_content),
                        current_style.unwrap_or_default(),
                    ));
                }
                if let Some(url) = current_url.take() {
                    links.push(TextLink {
                        line_index: line_index as u16,
                        start: link_start,
                        width: link_width,
                        url,
                    });
                }
                current_style = Some(cell.style);
                current_url = cell.url.clone();
                current_content.push(cell.symbol);
                link_start = cursor;
                link_width = 0;
            }

            if current_url.is_some() {
                link_width = link_width.saturating_add(width);
            }
            cursor = cursor.saturating_add(width);
        }

        if !current_content.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut current_content),
                current_style.unwrap_or_default(),
            ));
        }
        if let Some(url) = current_url.take() {
            links.push(TextLink {
                line_index: line_index as u16,
                start: link_start,
                width: link_width,
                url,
            });
        }

        rendered_lines.push(if spans.is_empty() {
            Line::default()
        } else {
            Line::from(spans)
        });
    }

    if rendered_lines.is_empty() {
        rendered_lines.push(Line::default());
    }

    RenderedTextBlock {
        text: Text::from(rendered_lines),
        links,
    }
}

fn prefix_cells(prefix: &str, style: Style) -> Vec<StyledCell> {
    prefix
        .chars()
        .map(|symbol| StyledCell {
            symbol,
            style,
            url: None,
        })
        .collect()
}

fn fragments_to_cells(fragments: &[InlineFragment]) -> Vec<StyledCell> {
    let mut cells = Vec::new();
    for fragment in fragments {
        cells.extend(fragment.content.chars().map(|symbol| StyledCell {
            symbol,
            style: fragment.style,
            url: fragment.url.clone(),
        }));
    }
    cells
}

fn wrap_content_cells(
    cells: &[StyledCell],
    first_width: u16,
    continuation_width: u16,
) -> Vec<Vec<StyledCell>> {
    if cells.is_empty() {
        return vec![Vec::new()];
    }

    let mut wrapped = Vec::new();
    let mut start = 0usize;
    let mut width_limit = first_width.max(1) as usize;

    while start < cells.len() {
        let remaining = &cells[start..];
        if remaining.len() <= width_limit {
            wrapped.push(remaining.to_vec());
            break;
        }

        let break_at = remaining[..width_limit]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, cell)| cell.symbol.is_whitespace())
            .map(|(index, _)| index + 1)
            .unwrap_or(width_limit);

        wrapped.push(remaining[..break_at].to_vec());
        start += break_at;
        width_limit = continuation_width.max(1) as usize;
    }

    if wrapped.is_empty() {
        wrapped.push(Vec::new());
    }

    wrapped
}

fn cell_width(symbol: char) -> u16 {
    u16::from(!symbol.is_control())
}

struct UrlMatch<'a> {
    visible: &'a str,
    trailing: &'a str,
    consumed: usize,
}

fn parse_url_prefix(source: &str) -> Option<UrlMatch<'_>> {
    let candidate = if source.starts_with("https://") || source.starts_with("http://") {
        source
            .find(char::is_whitespace)
            .map(|index| &source[..index])
            .unwrap_or(source)
    } else {
        return None;
    };

    let visible = candidate.trim_end_matches(is_trailing_url_punctuation);
    if visible == "https://" || visible == "http://" {
        return None;
    }

    Some(UrlMatch {
        visible,
        trailing: &candidate[visible.len()..],
        consumed: candidate.len(),
    })
}

fn is_trailing_url_punctuation(character: char) -> bool {
    matches!(character, '.' | ',' | '!' | '?' | ':' | ';' | ')')
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::render_markdown;
    use crate::tui::theme::{SurfaceTone, Theme};

    #[test]
    fn renders_headers_and_bold_text() {
        let theme = Theme::default();
        let rendered = render_markdown(&theme, "# Heading\n\nA **bold** move.", 40);

        assert_eq!(rendered.text.lines[0].spans[0].content.as_ref(), "Heading");
        assert!(
            rendered.text.lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert_eq!(rendered.text.lines[2].spans[1].content.as_ref(), "bold");
        assert!(
            rendered.text.lines[2].spans[1]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn heading_color_differs_from_general_notes_header_color() {
        let theme = Theme::default();
        let rendered = render_markdown(&theme, "# Heading", 40);

        assert_ne!(
            rendered.text.lines[0].spans[0].style.fg,
            theme.surface_title_style(SurfaceTone::Details).fg
        );
    }

    #[test]
    fn detects_styles_and_tracks_raw_urls() {
        let theme = Theme::default();
        let rendered = render_markdown(&theme, "Visit https://example.com/docs.", 40);

        assert_eq!(rendered.links.len(), 1);
        assert_eq!(rendered.links[0].url, "https://example.com/docs");
        assert_eq!(rendered.links[0].line_index, 0);
        assert_eq!(rendered.links[0].start, 6);
        assert_eq!(
            rendered.text.lines[0].spans[1].content.as_ref(),
            "https://example.com/docs"
        );
        assert!(
            rendered.text.lines[0].spans[1]
                .style
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
        assert_eq!(rendered.text.lines[0].spans[2].content.as_ref(), ".");
    }

    #[test]
    fn keeps_code_urls_out_of_link_tracking() {
        let theme = Theme::default();
        let rendered = render_markdown(&theme, "`https://example.com`", 40);

        assert!(rendered.links.is_empty());
        assert_eq!(
            rendered.text.lines[0].spans[0].content.as_ref(),
            "https://example.com"
        );
    }

    #[test]
    fn tracks_multiple_urls_on_one_line() {
        let theme = Theme::default();
        let rendered = render_markdown(
            &theme,
            "Docs https://example.com and https://openai.com",
            80,
        );

        assert_eq!(
            rendered
                .links
                .iter()
                .map(|link| link.url.as_str())
                .collect::<Vec<_>>(),
            vec!["https://example.com", "https://openai.com"]
        );
    }

    #[test]
    fn keeps_newline_separated_urls_as_distinct_links() {
        let theme = Theme::default();
        let rendered = render_markdown(&theme, "https://example.com\nhttps://openai.com", 80);

        assert_eq!(
            rendered
                .links
                .iter()
                .map(|link| (link.line_index, link.url.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "https://example.com"), (1, "https://openai.com")]
        );
    }
}
