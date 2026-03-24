use ratatui::style::{Color, Style};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub fg_default: Color,
    pub fg_muted: Color,
    pub fg_success: Color,
    pub fg_warning: Color,
    pub fg_error: Color,
    pub fg_accent: Color,
    pub bg_panel: Color,
    pub bg_selected: Color,
    pub bg_overlay: Color,
    pub border_default: Color,
    pub border_focus: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            fg_default: Color::White,
            fg_muted: Color::DarkGray,
            fg_success: Color::Green,
            fg_warning: Color::Yellow,
            fg_error: Color::Red,
            fg_accent: Color::Cyan,
            bg_panel: Color::Black,
            bg_selected: Color::Blue,
            bg_overlay: Color::DarkGray,
            border_default: Color::DarkGray,
            border_focus: Color::Cyan,
        }
    }
}

impl Theme {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let accent = color_from_name(&config.theme.accent).unwrap_or(Color::Cyan);

        match config.theme.mode.as_str() {
            "light" => Self {
                fg_default: Color::Black,
                fg_muted: Color::DarkGray,
                fg_success: Color::Green,
                fg_warning: Color::Yellow,
                fg_error: Color::Red,
                fg_accent: accent,
                bg_panel: Color::White,
                bg_selected: Color::Rgb(220, 235, 255),
                bg_overlay: Color::Rgb(245, 245, 245),
                border_default: Color::Gray,
                border_focus: accent,
            },
            _ => Self {
                fg_accent: accent,
                border_focus: accent,
                ..Self::default()
            },
        }
    }

    pub fn block_style(&self) -> Style {
        Style::default().fg(self.fg_default).bg(self.bg_panel)
    }

    pub fn selected_style(&self) -> Style {
        Style::default().fg(self.fg_default).bg(self.bg_selected)
    }
}

fn color_from_name(name: &str) -> Option<Color> {
    match name.to_ascii_lowercase().as_str() {
        "black" => Some(Color::Black),
        "blue" => Some(Color::Blue),
        "cyan" => Some(Color::Cyan),
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

    use super::Theme;
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
        assert_eq!(light.border_focus, Color::Red);
        assert_eq!(light.selected_style().bg, Some(light.bg_selected));
        assert_eq!(light.block_style().fg, Some(light.fg_default));
    }
}
