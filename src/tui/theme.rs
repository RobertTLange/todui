use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceTone {
    Neutral,
    Open,
    Completed,
    Details,
    Focus,
    Break,
    History,
    Overlay,
    Danger,
    Notice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTone {
    Default,
    Muted,
    Open,
    Completed,
    Focus,
    Break,
    Warning,
    Danger,
    Meta,
    Tag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTone {
    Neutral,
    Open,
    Completed,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub fg_default: Color,
    pub fg_muted: Color,
    pub fg_success: Color,
    pub fg_warning: Color,
    pub fg_error: Color,
    pub fg_accent: Color,
    pub bg_app: Color,
    pub bg_panel: Color,
    pub bg_overlay: Color,
    pub bg_notice: Color,
    pub bg_danger: Color,
    pub bg_open_selected: Color,
    pub bg_completed_selected: Color,
    pub bg_history_selected: Color,
    pub border_default: Color,
    pub border_focus: Color,
    accent_completed: Color,
    accent_details: Color,
    accent_focus: Color,
    accent_break: Color,
    accent_history: Color,
    accent_tag: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            fg_default: Color::Rgb(235, 241, 250),
            fg_muted: Color::Rgb(145, 157, 176),
            fg_success: Color::Rgb(124, 189, 121),
            fg_warning: Color::Rgb(240, 187, 96),
            fg_error: Color::Rgb(214, 96, 104),
            fg_accent: Color::Cyan,
            bg_app: Color::Rgb(8, 12, 18),
            bg_panel: Color::Rgb(12, 18, 28),
            bg_overlay: Color::Rgb(20, 26, 38),
            bg_notice: Color::Rgb(37, 29, 18),
            bg_danger: Color::Rgb(34, 15, 21),
            bg_open_selected: Color::Rgb(24, 60, 110),
            bg_completed_selected: Color::Rgb(74, 23, 29),
            bg_history_selected: Color::Rgb(82, 31, 39),
            border_default: Color::Rgb(65, 77, 96),
            border_focus: Color::Cyan,
            accent_completed: Color::Rgb(214, 96, 104),
            accent_details: Color::Rgb(224, 177, 92),
            accent_focus: Color::Rgb(116, 197, 131),
            accent_break: Color::Rgb(232, 154, 84),
            accent_history: Color::Rgb(196, 124, 132),
            accent_tag: Color::Rgb(154, 198, 228),
        }
    }
}

impl Theme {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let accent = color_from_name(&config.theme.accent).unwrap_or(Color::Cyan);

        match config.theme.mode.as_str() {
            "light" => Self {
                fg_default: Color::Black,
                fg_muted: Color::Rgb(87, 95, 108),
                fg_success: Color::Rgb(35, 122, 59),
                fg_warning: Color::Rgb(160, 103, 28),
                fg_error: Color::Rgb(182, 57, 72),
                fg_accent: accent,
                bg_app: Color::Rgb(245, 247, 251),
                bg_panel: Color::Rgb(255, 255, 255),
                bg_overlay: Color::Rgb(236, 240, 247),
                bg_notice: Color::Rgb(255, 247, 226),
                bg_danger: Color::Rgb(255, 235, 238),
                bg_open_selected: Color::Rgb(208, 227, 247),
                bg_completed_selected: Color::Rgb(245, 218, 220),
                bg_history_selected: Color::Rgb(244, 225, 228),
                border_default: Color::Rgb(144, 151, 167),
                border_focus: accent,
                accent_completed: Color::Rgb(182, 57, 72),
                accent_details: Color::Rgb(162, 99, 28),
                accent_focus: Color::Rgb(35, 122, 59),
                accent_break: Color::Rgb(191, 116, 24),
                accent_history: Color::Rgb(161, 82, 93),
                accent_tag: Color::Rgb(73, 120, 176),
            },
            _ => Self {
                fg_accent: accent,
                border_focus: accent,
                ..Self::default()
            },
        }
    }

    pub fn app_style(&self) -> Style {
        Style::default().fg(self.fg_default).bg(self.bg_app)
    }

    pub fn block_style(&self) -> Style {
        self.surface_style(SurfaceTone::Neutral)
    }

    pub fn selected_style(&self) -> Style {
        self.selection_style(SelectionTone::Open)
    }

    pub fn surface_style(&self, tone: SurfaceTone) -> Style {
        let background = match tone {
            SurfaceTone::Overlay => self.bg_overlay,
            SurfaceTone::Danger => self.bg_danger,
            SurfaceTone::Notice => self.bg_notice,
            _ => self.bg_panel,
        };

        Style::default().fg(self.fg_default).bg(background)
    }

    pub fn surface_border_style(&self, tone: SurfaceTone) -> Style {
        let color = match tone {
            SurfaceTone::Neutral => self.border_default,
            SurfaceTone::Open => self.fg_accent,
            SurfaceTone::Completed => self.accent_completed,
            SurfaceTone::Details => self.accent_details,
            SurfaceTone::Focus => self.accent_focus,
            SurfaceTone::Break => self.accent_break,
            SurfaceTone::History => self.accent_history,
            SurfaceTone::Overlay => self.border_focus,
            SurfaceTone::Danger => self.fg_error,
            SurfaceTone::Notice => self.fg_warning,
        };

        Style::default().fg(color)
    }

    pub fn surface_title_style(&self, tone: SurfaceTone) -> Style {
        self.surface_border_style(tone).add_modifier(Modifier::BOLD)
    }

    pub fn selection_style(&self, tone: SelectionTone) -> Style {
        let background = match tone {
            SelectionTone::Neutral => self.bg_overlay,
            SelectionTone::Open => self.bg_open_selected,
            SelectionTone::Completed => self.bg_completed_selected,
            SelectionTone::History => self.bg_history_selected,
        };

        Style::default()
            .fg(self.fg_default)
            .bg(background)
            .add_modifier(Modifier::BOLD)
    }

    pub fn text_style(&self, tone: TextTone) -> Style {
        let color = match tone {
            TextTone::Default => self.fg_default,
            TextTone::Muted => self.fg_muted,
            TextTone::Open => self.fg_accent,
            TextTone::Completed => self.accent_completed,
            TextTone::Focus => self.accent_focus,
            TextTone::Break => self.accent_break,
            TextTone::Warning => self.fg_warning,
            TextTone::Danger => self.fg_error,
            TextTone::Meta => self.accent_details,
            TextTone::Tag => self.accent_tag,
        };

        Style::default().fg(color)
    }
}

fn color_from_name(name: &str) -> Option<Color> {
    match name.to_ascii_lowercase().as_str() {
        "black" => Some(Color::Black),
        "blue" => Some(Color::Blue),
        "cyan" => Some(Color::Cyan),
        "gray" => Some(Color::Gray),
        "green" => Some(Color::Green),
        "magenta" => Some(Color::Magenta),
        "red" => Some(Color::Red),
        "white" => Some(Color::White),
        "yellow" => Some(Color::Yellow),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{SelectionTone, SurfaceTone, TextTone, Theme};
    use crate::config::Config;

    #[test]
    fn builds_dark_and_light_themes_from_config() {
        let dark = Theme::from_config(&Config::default());
        assert_eq!(dark.fg_accent, Color::Cyan);

        let mut config = Config::default();
        config.theme.mode = String::from("light");
        config.theme.accent = String::from("red");
        let light = Theme::from_config(&config);

        assert_eq!(light.fg_default, Color::Black);
        assert_eq!(
            light.surface_border_style(SurfaceTone::Open).fg,
            Some(Color::Red)
        );
        assert_eq!(
            light.surface_style(SurfaceTone::Neutral).fg,
            Some(light.fg_default)
        );
        assert_ne!(
            light.selection_style(SelectionTone::Open).bg,
            light.selection_style(SelectionTone::Completed).bg
        );
    }

    #[test]
    fn accent_only_drives_open_chrome_not_completed_tone() {
        let mut config = Config::default();
        config.theme.accent = String::from("green");

        let theme = Theme::from_config(&config);

        assert_eq!(
            theme.surface_border_style(SurfaceTone::Open).fg,
            Some(Color::Green)
        );
        assert_ne!(
            theme.surface_border_style(SurfaceTone::Completed).fg,
            Some(Color::Green)
        );
        assert_eq!(
            theme.text_style(TextTone::Completed).fg,
            theme.surface_border_style(SurfaceTone::Completed).fg
        );
    }

    #[test]
    fn completed_selection_uses_fixed_red_toning() {
        let theme = Theme::from_config(&Config::default());

        assert_eq!(
            theme.selection_style(SelectionTone::Completed).bg,
            Some(Color::Rgb(74, 23, 29))
        );
        assert_eq!(
            theme.selection_style(SelectionTone::Open).bg,
            Some(Color::Rgb(24, 60, 110))
        );
    }
}
