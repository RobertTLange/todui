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
    pub list: Rect,
    pub footer: Option<Rect>,
}

pub fn layout_mode(width: u16) -> LayoutMode {
    match width {
        0..=49 => LayoutMode::Narrow,
        50..=99 => LayoutMode::Medium,
        _ => LayoutMode::Wide,
    }
}

pub fn split_screen(area: Rect, top_bar_height: u16, footer_height: Option<u16>) -> ScreenLayout {
    let mode = layout_mode(area.width);
    let outer = Layout::vertical([
        Constraint::Length(top_bar_height.max(3)),
        Constraint::Min(0),
    ])
    .split(area);

    match mode {
        _ if footer_height.is_some() => {
            let footer_height = footer_height.unwrap_or(0).max(3);
            let panes = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(footer_height.min(outer[1].height)),
            ])
            .split(outer[1]);
            ScreenLayout {
                mode,
                top_bar: outer[0],
                main: outer[1],
                list: panes[0],
                footer: Some(panes[1]),
            }
        }
        _ => ScreenLayout {
            mode,
            top_bar: outer[0],
            main: outer[1],
            list: outer[1],
            footer: None,
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
    fn wide_layout_uses_requested_footer_height() {
        let layout = split_screen(Rect::new(0, 0, 120, 24), 3, Some(4));
        assert_eq!(layout.footer.expect("footer").height, 4);
    }

    #[test]
    fn medium_layout_uses_requested_footer_height() {
        let layout = split_screen(Rect::new(0, 0, 80, 24), 3, Some(4));
        assert_eq!(layout.footer.expect("footer").height, 4);
    }

    #[test]
    fn layout_hides_footer_when_not_requested() {
        let layout = split_screen(Rect::new(0, 0, 120, 24), 3, None);
        assert!(layout.footer.is_none());
    }

    #[test]
    fn narrow_layout_uses_requested_footer_height() {
        let layout = split_screen(Rect::new(0, 0, 49, 24), 3, Some(4));
        assert_eq!(layout.footer.expect("footer").height, 4);
    }
}
