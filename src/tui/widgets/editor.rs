use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::theme::{SurfaceTone, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorField {
    #[default]
    Primary,
    Secondary,
    Tertiary,
}

pub struct EditorView<'a> {
    pub title: &'a str,
    pub primary_label: &'a str,
    pub primary_value: &'a str,
    pub primary_cursor: Option<usize>,
    pub secondary_label: Option<&'a str>,
    pub secondary_value: Option<&'a str>,
    pub secondary_cursor: Option<usize>,
    pub tertiary_label: Option<&'a str>,
    pub tertiary_value: Option<&'a str>,
    pub tertiary_cursor: Option<usize>,
    pub tertiary_value_style: Option<Style>,
    pub focused_field: EditorField,
    pub error: Option<&'a str>,
    pub footer_hint: &'a str,
}

pub fn render_editor<'a>(theme: &Theme, view: EditorView<'a>, width: u16) -> Paragraph<'a> {
    Paragraph::new(editor_content_lines(&view, width.saturating_sub(2)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(view.title)
                .style(theme.surface_style(SurfaceTone::Overlay))
                .border_style(theme.surface_border_style(SurfaceTone::Overlay))
                .title_style(theme.surface_title_style(SurfaceTone::Overlay)),
        )
        .style(Style::default().fg(theme.fg_default).bg(theme.bg_overlay))
}

pub fn editor_height(view: &EditorView<'_>, width: u16) -> u16 {
    editor_content_lines(view, width.saturating_sub(2))
        .len()
        .saturating_add(2)
        .max(10) as u16
}

fn editor_content_lines(view: &EditorView<'_>, inner_width: u16) -> Vec<Line<'static>> {
    let mut lines = field_lines(
        view.primary_label,
        view.primary_value,
        matches!(view.focused_field, EditorField::Primary),
        view.primary_cursor,
        inner_width,
    );

    if let (Some(label), Some(value)) = (view.secondary_label, view.secondary_value) {
        lines.push(Line::from(String::new()));
        lines.extend(field_lines(
            label,
            value,
            matches!(view.focused_field, EditorField::Secondary),
            view.secondary_cursor,
            inner_width,
        ));
    }

    if let (Some(label), Some(value)) = (view.tertiary_label, view.tertiary_value) {
        lines.push(Line::from(String::new()));
        lines.extend(styled_field_lines(
            label,
            value,
            matches!(view.focused_field, EditorField::Tertiary),
            view.tertiary_cursor,
            view.tertiary_value_style,
            inner_width,
        ));
    }

    lines.push(Line::from(String::new()));
    if let Some(error) = view.error {
        lines.extend(wrapped_plain_lines(&format!("Error: {error}"), inner_width));
        lines.push(Line::from(String::new()));
    }
    lines.extend(wrapped_plain_lines(view.footer_hint, inner_width));
    lines
}

fn display_field(value: &str, focused: bool, cursor: Option<usize>) -> String {
    let mut display = normalize_line_breaks(value);
    if focused {
        let cursor = cursor.unwrap_or(display.len());
        insert_cursor_marker(&mut display, cursor);
    }
    display
}

fn normalize_line_breaks(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn insert_cursor_marker(display: &mut String, cursor: usize) {
    let mut cursor = cursor.min(display.len());
    while cursor > 0 && !display.is_char_boundary(cursor) {
        cursor -= 1;
    }
    display.insert(cursor, '|');
}

fn styled_field_lines(
    label: &str,
    value: &str,
    focused: bool,
    cursor: Option<usize>,
    value_style: Option<Style>,
    inner_width: u16,
) -> Vec<Line<'static>> {
    let display = display_field(value, focused, cursor);
    let mut display_lines = display.split('\n');
    let first = display_lines.next().unwrap_or_default();
    let continuation_indent = " ".repeat(label.chars().count() + 2);
    let first_width = available_width(inner_width, label.chars().count() + 2);
    let continuation_width = available_width(inner_width, continuation_indent.chars().count());

    let mut lines =
        wrapped_field_line(label, first, value_style, first_width, &continuation_indent);
    for line in display_lines {
        lines.extend(wrapped_continuation_line(
            &continuation_indent,
            line,
            value_style,
            continuation_width,
        ));
    }
    lines
}

fn field_lines(
    label: &str,
    value: &str,
    focused: bool,
    cursor: Option<usize>,
    inner_width: u16,
) -> Vec<Line<'static>> {
    styled_field_lines(label, value, focused, cursor, None, inner_width)
}

fn available_width(inner_width: u16, prefix_width: usize) -> usize {
    usize::from(inner_width).saturating_sub(prefix_width).max(1)
}

fn wrapped_field_line(
    label: &str,
    value: &str,
    value_style: Option<Style>,
    available_width: usize,
    continuation_indent: &str,
) -> Vec<Line<'static>> {
    let mut segments = wrap_segments(value, available_width).into_iter();
    let first = segments.next().unwrap_or_default();

    let mut lines = vec![styled_line(label, &first, value_style)];
    lines.extend(
        segments
            .map(|segment| styled_continuation_line(continuation_indent, &segment, value_style)),
    );
    lines
}

fn wrapped_continuation_line(
    indent: &str,
    value: &str,
    value_style: Option<Style>,
    available_width: usize,
) -> Vec<Line<'static>> {
    wrap_segments(value, available_width)
        .into_iter()
        .map(|segment| styled_continuation_line(indent, &segment, value_style))
        .collect()
}

fn wrapped_plain_lines(value: &str, inner_width: u16) -> Vec<Line<'static>> {
    let width = usize::from(inner_width).max(1);
    normalize_line_breaks(value)
        .split('\n')
        .flat_map(|line| wrap_segments(line, width).into_iter())
        .map(Line::from)
        .collect()
}

fn wrap_segments(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for character in value.chars() {
        if current_width == width {
            segments.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(character);
        current_width += 1;
    }

    if current.is_empty() {
        segments.push(String::new());
    } else {
        segments.push(current);
    }

    segments
}

fn styled_line(label: &str, value: &str, value_style: Option<Style>) -> Line<'static> {
    match value_style {
        Some(style) => Line::from(vec![
            Span::raw(format!("{label}: ")),
            Span::styled(value.to_string(), style),
        ]),
        None => Line::from(format!("{label}: {value}")),
    }
}

fn styled_continuation_line(
    indent: &str,
    value: &str,
    value_style: Option<Style>,
) -> Line<'static> {
    match value_style {
        Some(style) => Line::from(vec![
            Span::raw(indent.to_string()),
            Span::styled(value.to_string(), style),
        ]),
        None => Line::from(format!("{indent}{value}")),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Modifier;

    use super::{EditorField, EditorView, render_editor};
    use crate::tui::theme::Theme;

    #[test]
    fn editor_marks_focused_field_and_error() {
        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    render_editor(
                        &Theme::default(),
                        EditorView {
                            title: "New Todo",
                            primary_label: "Title",
                            primary_value: "Draft spec",
                            primary_cursor: Some("Draft spec".len()),
                            secondary_label: Some("Notes"),
                            secondary_value: Some("cover TUI"),
                            secondary_cursor: Some("cover TUI".len()),
                            tertiary_label: Some("Repo"),
                            tertiary_value: Some("@sakanaai/todui-keymove"),
                            tertiary_cursor: Some("@sakanaai/todui-keymove".len()),
                            tertiary_value_style: None,
                            focused_field: EditorField::Tertiary,
                            error: Some("Todo title is required"),
                            footer_hint: "Enter save  Esc cancel",
                        },
                        frame.area().width,
                    ),
                    frame.area(),
                );
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Title: Draft spec"));
        assert!(text.contains("Notes: cover TUI"));
        assert!(text.contains("Repo: @sakanaai/todui-keymove|"));
        assert!(text.contains("Todo title is required"));
    }

    #[test]
    fn editor_renders_multiline_field_values_with_cursor_on_next_line() {
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    render_editor(
                        &Theme::default(),
                        EditorView {
                            title: "New Todo",
                            primary_label: "Title",
                            primary_value: "Draft spec",
                            primary_cursor: Some("Draft spec".len()),
                            secondary_label: Some("Notes"),
                            secondary_value: Some("first line\nsecond line"),
                            secondary_cursor: Some("first line\nsecond line".len()),
                            tertiary_label: None,
                            tertiary_value: None,
                            tertiary_cursor: None,
                            tertiary_value_style: None,
                            focused_field: EditorField::Secondary,
                            error: None,
                            footer_hint: "Enter save  Esc cancel",
                        },
                        frame.area().width,
                    ),
                    frame.area(),
                );
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Notes: first line"));
        assert!(text.contains("       second line|"));
    }

    #[test]
    fn editor_places_cursor_inside_focused_field() {
        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    render_editor(
                        &Theme::default(),
                        EditorView {
                            title: "Edit Todo",
                            primary_label: "Title",
                            primary_value: "Draftspec",
                            primary_cursor: Some(5),
                            secondary_label: None,
                            secondary_value: None,
                            secondary_cursor: None,
                            tertiary_label: None,
                            tertiary_value: None,
                            tertiary_cursor: None,
                            tertiary_value_style: None,
                            focused_field: EditorField::Primary,
                            error: None,
                            footer_hint: "Enter save  Esc cancel",
                        },
                        frame.area().width,
                    ),
                    frame.area(),
                );
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Title: Draft|spec"));
    }

    #[test]
    fn editor_keeps_hanging_indent_for_wrapped_multiline_notes() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    render_editor(
                        &Theme::default(),
                        EditorView {
                            title: "Edit Todo",
                            primary_label: "Title",
                            primary_value: "Prep talks",
                            primary_cursor: Some("Prep talks".len()),
                            secondary_label: Some("Notes"),
                            secondary_value: Some(
                                "alpha\n1234567890123456789012345678901TAIL\nbeta",
                            ),
                            secondary_cursor: Some(
                                "alpha\n1234567890123456789012345678901TAIL\nbeta".len(),
                            ),
                            tertiary_label: Some("Repo"),
                            tertiary_value: Some("sakanaai/shinkaevolve"),
                            tertiary_cursor: Some("sakanaai/shinkaevolve".len()),
                            tertiary_value_style: None,
                            focused_field: EditorField::Secondary,
                            error: None,
                            footer_hint: "Enter save  Esc cancel",
                        },
                        frame.area().width,
                    ),
                    frame.area(),
                );
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("       TAIL"));
        assert!(text.contains("       beta|"));
    }

    #[test]
    fn editor_styles_repo_value_as_link() {
        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    render_editor(
                        &Theme::default(),
                        EditorView {
                            title: "Edit Todo",
                            primary_label: "Title",
                            primary_value: "Draft spec",
                            primary_cursor: Some("Draft spec".len()),
                            secondary_label: None,
                            secondary_value: None,
                            secondary_cursor: None,
                            tertiary_label: Some("Repo"),
                            tertiary_value: Some("openai/codex"),
                            tertiary_cursor: Some("openai/codex".len()),
                            tertiary_value_style: Some(
                                Theme::default()
                                    .text_style(crate::tui::theme::TextTone::Open)
                                    .add_modifier(Modifier::UNDERLINED),
                            ),
                            focused_field: EditorField::Primary,
                            error: None,
                            footer_hint: "Enter save  Esc cancel",
                        },
                        frame.area().width,
                    ),
                    frame.area(),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let text = buffer_to_string(buffer);
        let row = text
            .lines()
            .position(|line| line.contains("Repo: openai/codex"))
            .expect("repo line");
        let column = text
            .lines()
            .nth(row)
            .and_then(|line| line.find("openai/codex"))
            .expect("repo column");

        assert!(
            buffer[(column as u16, row as u16)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    fn buffer_to_string(buffer: &Buffer) -> String {
        let mut lines = Vec::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line);
        }
        lines.join("\n")
    }
}
