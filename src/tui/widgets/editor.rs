use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

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
    pub secondary_label: Option<&'a str>,
    pub secondary_value: Option<&'a str>,
    pub tertiary_label: Option<&'a str>,
    pub tertiary_value: Option<&'a str>,
    pub tertiary_value_style: Option<Style>,
    pub focused_field: EditorField,
    pub error: Option<&'a str>,
    pub footer_hint: &'a str,
}

pub fn render_editor<'a>(theme: &Theme, view: EditorView<'a>) -> Paragraph<'a> {
    let mut lines = field_lines(
        view.primary_label,
        view.primary_value,
        matches!(view.focused_field, EditorField::Primary),
    );

    if let (Some(label), Some(value)) = (view.secondary_label, view.secondary_value) {
        lines.push(Line::from(String::new()));
        lines.extend(field_lines(
            label,
            value,
            matches!(view.focused_field, EditorField::Secondary),
        ));
    }

    if let (Some(label), Some(value)) = (view.tertiary_label, view.tertiary_value) {
        lines.push(Line::from(String::new()));
        lines.extend(styled_field_lines(
            label,
            value,
            matches!(view.focused_field, EditorField::Tertiary),
            view.tertiary_value_style,
        ));
    }

    lines.push(Line::from(String::new()));
    if let Some(error) = view.error {
        lines.push(Line::from(format!("Error: {error}")));
        lines.push(Line::from(String::new()));
    }
    lines.push(Line::from(view.footer_hint.to_string()));

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
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

fn display_field(value: &str, focused: bool) -> String {
    if focused {
        format!("{value}|")
    } else {
        value.to_string()
    }
}

fn field_lines(label: &str, value: &str, focused: bool) -> Vec<Line<'static>> {
    styled_field_lines(label, value, focused, None)
}

fn styled_field_lines(
    label: &str,
    value: &str,
    focused: bool,
    value_style: Option<Style>,
) -> Vec<Line<'static>> {
    let display = display_field(value, focused);
    let mut display_lines = display.lines();
    let first = display_lines.next().unwrap_or_default();
    let continuation_indent = " ".repeat(label.chars().count() + 2);

    let mut lines = vec![styled_line(label, first, value_style)];
    lines.extend(
        display_lines.map(|line| styled_continuation_line(&continuation_indent, line, value_style)),
    );
    lines
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
                            secondary_label: Some("Notes"),
                            secondary_value: Some("cover TUI"),
                            tertiary_label: Some("Repo"),
                            tertiary_value: Some("@sakanaai/todui-keymove"),
                            tertiary_value_style: None,
                            focused_field: EditorField::Tertiary,
                            error: Some("Todo title is required"),
                            footer_hint: "Enter save  Esc cancel",
                        },
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
                            secondary_label: Some("Notes"),
                            secondary_value: Some("first line\nsecond line"),
                            tertiary_label: None,
                            tertiary_value: None,
                            tertiary_value_style: None,
                            focused_field: EditorField::Secondary,
                            error: None,
                            footer_hint: "Enter save  Esc cancel",
                        },
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
                            secondary_label: None,
                            secondary_value: None,
                            tertiary_label: Some("Repo"),
                            tertiary_value: Some("openai/codex"),
                            tertiary_value_style: Some(
                                Theme::default()
                                    .text_style(crate::tui::theme::TextTone::Open)
                                    .add_modifier(Modifier::UNDERLINED),
                            ),
                            focused_field: EditorField::Primary,
                            error: None,
                            footer_hint: "Enter save  Esc cancel",
                        },
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
