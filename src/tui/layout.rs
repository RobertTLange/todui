use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Wide,
    Medium,
    Narrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenLayout {
    pub mode: LayoutMode,
    pub top_bar: Rect,
    pub main: Rect,
    pub footer: Rect,
    pub list: Rect,
    pub details: Option<Rect>,
    pub pomodoro: Option<Rect>,
}

pub fn layout_mode(width: u16) -> LayoutMode {
    match width {
        0..=49 => LayoutMode::Narrow,
        50..=99 => LayoutMode::Medium,
        _ => LayoutMode::Wide,
    }
}

pub fn split_screen(
    area: Rect,
    medium_drawer_open: bool,
    top_bar_height: u16,
    pomodoro_height: u16,
) -> ScreenLayout {
    let mode = layout_mode(area.width);
    let pomodoro_height = pomodoro_height.max(3);
    let outer = Layout::vertical([
        Constraint::Length(top_bar_height.max(3)),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(area);

    match mode {
        LayoutMode::Wide => {
            let panes =
                Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                    .split(outer[1]);
            let right = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(pomodoro_height.min(panes[1].height)),
            ])
            .split(panes[1]);
            ScreenLayout {
                mode,
                top_bar: outer[0],
                main: outer[1],
                footer: outer[2],
                list: panes[0],
                details: Some(right[0]),
                pomodoro: Some(right[1]),
            }
        }
        LayoutMode::Medium => {
            let panes = if medium_drawer_open {
                Layout::vertical([Constraint::Min(0), Constraint::Length(11)]).split(outer[1])
            } else {
                Layout::vertical([Constraint::Min(0), Constraint::Length(0)]).split(outer[1])
            };
            let (details, pomodoro) = if medium_drawer_open {
                let drawer = Layout::vertical([
                    Constraint::Min(0),
                    Constraint::Length(pomodoro_height.min(panes[1].height)),
                ])
                .split(panes[1]);
                (Some(drawer[0]), Some(drawer[1]))
            } else {
                (None, None)
            };
            ScreenLayout {
                mode,
                top_bar: outer[0],
                main: outer[1],
                footer: outer[2],
                list: panes[0],
                details,
                pomodoro,
            }
        }
        LayoutMode::Narrow => ScreenLayout {
            mode,
            top_bar: outer[0],
            main: outer[1],
            footer: outer[2],
            list: outer[1],
            details: None,
            pomodoro: None,
        },
    }
}

pub fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{LayoutMode, centered_rect, layout_mode, split_screen};

    #[test]
    fn selects_expected_layout_modes() {
        assert_eq!(layout_mode(49), LayoutMode::Narrow);
        assert_eq!(layout_mode(50), LayoutMode::Medium);
        assert_eq!(layout_mode(60), LayoutMode::Medium);
        assert_eq!(layout_mode(80), LayoutMode::Medium);
        assert_eq!(layout_mode(120), LayoutMode::Wide);
    }

    #[test]
    fn centers_overlay_inside_bounds() {
        let rect = centered_rect(Rect::new(0, 0, 100, 40), 40, 12);
        assert_eq!(rect.width, 40);
        assert_eq!(rect.height, 12);
    }

    #[test]
    fn medium_layout_can_hide_details_drawer() {
        let layout = split_screen(Rect::new(0, 0, 80, 24), false, 3, 4);
        assert!(layout.details.is_none());
    }

    #[test]
    fn wide_layout_uses_requested_pomodoro_height() {
        let layout = split_screen(Rect::new(0, 0, 120, 24), false, 3, 4);
        assert_eq!(layout.pomodoro.expect("pomodoro").height, 4);
    }

    #[test]
    fn medium_layout_uses_requested_pomodoro_height() {
        let layout = split_screen(Rect::new(0, 0, 80, 24), true, 3, 4);
        assert_eq!(layout.pomodoro.expect("pomodoro").height, 4);
    }
}
