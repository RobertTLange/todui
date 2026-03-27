use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorField {
    #[default]
    Primary,
    Secondary,
}

pub struct EditorView<'a> {
    pub title: &'a str,
    pub primary_label: &'a str,
    pub primary_value: &'a str,
    pub secondary_label: Option<&'a str>,
    pub secondary_value: Option<&'a str>,
    pub focused_field: EditorField,
    pub error: Option<&'a str>,
    pub footer_hint: &'a str,
}

pub fn render_editor<'a>(theme: &Theme, view: EditorView<'a>) -> Paragraph<'a> {
    let mut lines = vec![Line::from(format!(
        "{}: {}",
        view.primary_label,
        display_field(
            view.primary_value,
            matches!(view.focused_field, EditorField::Primary)
        )
    ))];

    if let (Some(label), Some(value)) = (view.secondary_label, view.secondary_value) {
        lines.push(Line::from(String::new()));
        lines.push(Line::from(format!(
            "{}: {}",
            label,
            display_field(value, matches!(view.focused_field, EditorField::Secondary))
        )));
    }

    lines.push(Line::from(String::new()));
    if let Some(error) = view.error {
        lines.push(Line::from(format!("Error: {error}")));
        lines.push(Line::from(String::new()));
    }
    lines.push(Line::from(view.footer_hint.to_string()));

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title(view.title))
        .style(Style::default().fg(theme.fg_default).bg(theme.bg_overlay))
}

fn display_field(value: &str, focused: bool) -> String {
    if focused {
        format!("{value}|")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

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
                            focused_field: EditorField::Secondary,
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
        assert!(text.contains("Notes: cover TUI|"));
        assert!(text.contains("Todo title is required"));
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
