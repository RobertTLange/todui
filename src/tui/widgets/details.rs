use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::tui::theme::{TextTone, Theme};

const REPO_LABEL: &str = "repo: ";

pub(crate) fn repo_value_style(theme: &Theme) -> Style {
    theme
        .text_style(TextTone::Open)
        .add_modifier(Modifier::UNDERLINED)
}

pub(crate) fn repo_line(theme: &Theme, repo: Option<&str>) -> Line<'static> {
    let value = repo.unwrap_or("-");
    let style = if repo.is_some() {
        repo_value_style(theme)
    } else {
        theme.text_style(TextTone::Muted)
    };

    Line::from(vec![
        Span::raw(REPO_LABEL),
        Span::styled(value.to_string(), style),
    ])
}

pub(crate) fn repo_hitbox(area: Rect, line_index: u16, repo: Option<&str>) -> Option<Rect> {
    let repo = repo?;
    if repo.is_empty() || area.width <= 2 || area.height <= 2 {
        return None;
    }

    let x = area
        .x
        .saturating_add(1)
        .saturating_add(REPO_LABEL.len() as u16);
    let y = area.y.saturating_add(1).saturating_add(line_index);
    let inner_right = area.x.saturating_add(area.width).saturating_sub(1);
    let available_width = inner_right.saturating_sub(x);
    let width = (repo.len() as u16).min(available_width);

    if width == 0 || y >= area.bottom().saturating_sub(1) {
        return None;
    }

    Some(Rect::new(x, y, width, 1))
}

pub(crate) fn rect_contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}
