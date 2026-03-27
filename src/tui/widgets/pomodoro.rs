use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::domain::pomodoro::{PomodoroRun, PomodoroState, progress_ratio, remaining_seconds};
use crate::tui::theme::{SurfaceTone, TextTone, Theme};

const FOOTER_LINES: u16 = 2;

pub fn active_footer_height() -> u16 {
    FOOTER_LINES + 2
}

pub fn active_footer(theme: &Theme, run: &PomodoroRun, now: i64) -> Paragraph<'static> {
    Paragraph::new(active_footer_lines(theme, run, now))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Pomodoro")
                .style(theme.surface_style(SurfaceTone::Neutral))
                .border_style(theme.surface_border_style(SurfaceTone::Focus))
                .title_style(theme.surface_title_style(SurfaceTone::Focus)),
        )
        .style(theme.surface_style(SurfaceTone::Neutral))
}

fn active_footer_lines(theme: &Theme, run: &PomodoroRun, now: i64) -> Vec<Line<'static>> {
    vec![
        Line::styled(status_line(run, now), theme.text_style(TextTone::Focus)),
        Line::styled(progress_bar(run, now), theme.text_style(TextTone::Focus)),
    ]
}

fn status_line(run: &PomodoroRun, now: i64) -> String {
    let remaining = format_duration(remaining_seconds(run, now));
    match run.state {
        PomodoroState::Paused => format!("{} · paused · {remaining} remaining", run.kind.label()),
        _ => format!("{} · {remaining} remaining", run.kind.label()),
    }
}

fn format_duration(seconds: i64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn progress_bar(run: &PomodoroRun, now: i64) -> String {
    let ratio = progress_ratio(run, now);
    let filled = (ratio * 20.0).round() as usize;
    let empty = 20_usize.saturating_sub(filled);
    format!(
        "{}{} {:>3}%",
        "█".repeat(filled),
        "░".repeat(empty),
        (ratio * 100.0) as u32
    )
}
