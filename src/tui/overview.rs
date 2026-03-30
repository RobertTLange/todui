use std::cmp::min;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::config::Config;
use crate::db::Database;
use crate::domain::pomodoro::{PomodoroKind, PomodoroRun, PomodoroState, remaining_seconds};
use crate::domain::session::SessionOverview;
use crate::domain::todo::{Todo, TodoStatus};
use crate::error::Result;
use crate::timestamp::now_utc_timestamp;
use crate::timestamp::{format_full_local, format_month_day_local};
use crate::tui::layout::centered_rect;
use crate::tui::terminal::AppTerminal;
use crate::tui::theme::{SelectionTone, SurfaceTone, TextTone, Theme};
use crate::tui::widgets::editor::{EditorField, EditorView, render_editor};
use crate::tui::widgets::markdown::render_markdown;
use crate::tui::widgets::pomodoro::{active_footer, active_footer_height};

const EVENT_POLL_MS: u64 = 250;
const TAG_COLUMN_WIDTH: usize = 10;
const REV_COLUMN_WIDTH: usize = 5;
const OPEN_COLUMN_WIDTH: usize = 5;
const DONE_COLUMN_WIDTH: usize = 5;
const LAST_OPENED_COLUMN_WIDTH: usize = 11;
const SESSION_COLUMN_SPACING: usize = 5;
const TODO_PREVIEW_TIME_WIDTH: usize = 11;
const NOTES_EDITOR_WIDTH: u16 = 72;
const NOTES_EDITOR_HEIGHT: u16 = 18;
const OVERVIEW_LIST_PERCENT: u16 = 40;
const OVERVIEW_NOTES_PERCENT: u16 = 40;
const OVERVIEW_SUMMARY_PERCENT: u16 = 20;

pub fn run(database: &mut Database, config: &Config) -> Result<()> {
    super::run(database, config, super::TuiRoute::Overview)
}

pub(crate) fn run_in_terminal(
    terminal: &mut AppTerminal,
    database: &mut Database,
    config: &Config,
) -> Result<OverviewExit> {
    let mut screen = OverviewScreen::new(config.clone());
    screen.reload(database)?;

    loop {
        terminal.draw(|frame| screen.render(frame))?;

        if event::poll(Duration::from_millis(EVENT_POLL_MS))? {
            match event::read()? {
                Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                    if let Some(exit) = screen.handle_key(database, key_event)? {
                        break Ok(exit);
                    }
                }
                Event::Mouse(mouse_event) => {
                    if let Some(exit) = screen.handle_mouse(mouse_event) {
                        break Ok(exit);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        } else {
            screen.handle_tick(database)?;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverviewExit {
    Quit,
    OpenSession(String),
}

#[derive(Debug)]
struct OverviewScreen {
    sessions: Vec<SessionOverview>,
    expanded_sessions: Vec<ExpandedSessionState>,
    active_run: Option<PomodoroRun>,
    overview_notes: String,
    has_any_sessions: bool,
    selected_index: usize,
    scroll_offset: usize,
    theme: Theme,
    config: Config,
    last_area: Rect,
    overlay: Option<OverviewOverlay>,
    session_editor: SessionEditorState,
    notes_editor: GeneralNotesEditorState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OverviewOverlay {
    Help,
    SessionEditor(SessionEditorMode),
    GeneralNotesEditor,
    SessionMetadata,
    DeleteSession { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionEditorMode {
    Create,
    EditMetadata { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpandedSessionState {
    name: String,
    todos: Vec<Todo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverviewDisplayRow {
    Session(usize),
    Todo {
        session_index: usize,
        todo_index: usize,
    },
    EmptyTodos(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionEditorState {
    name: String,
    tag: String,
    repo: String,
    focused_field: EditorField,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct GeneralNotesEditorState {
    text: String,
}

impl Default for SessionEditorState {
    fn default() -> Self {
        Self {
            name: String::new(),
            tag: String::new(),
            repo: String::new(),
            focused_field: EditorField::Primary,
            error: None,
        }
    }
}

impl OverviewScreen {
    fn new(config: Config) -> Self {
        Self {
            sessions: Vec::new(),
            expanded_sessions: Vec::new(),
            active_run: None,
            overview_notes: String::new(),
            has_any_sessions: false,
            selected_index: 0,
            scroll_offset: 0,
            theme: Theme::from_config(&config),
            config,
            last_area: Rect::default(),
            overlay: None,
            session_editor: SessionEditorState::default(),
            notes_editor: GeneralNotesEditorState::default(),
        }
    }

    fn reload(&mut self, database: &Database) -> Result<()> {
        self.has_any_sessions = database.has_any_sessions()?;
        self.sessions = database.list_session_overview()?;
        self.active_run = database.get_active_pomodoro()?;
        self.overview_notes = database.get_overview_notes()?;
        self.selected_index = min(self.selected_index, self.sessions.len().saturating_sub(1));
        self.sync_expanded_sessions(database)?;
        self.ensure_selection_visible(self.visible_rows());
        Ok(())
    }

    fn handle_tick(&mut self, database: &mut Database) -> Result<()> {
        if let Some(run) = self.active_run.clone()
            && matches!(run.state, PomodoroState::Running)
            && remaining_seconds(&run, now_utc_timestamp()) == 0
        {
            database.complete_pomodoro(run.id, now_utc_timestamp())?;
        }
        self.reload(database)
    }

    fn handle_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<OverviewExit>> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Ok(Some(OverviewExit::Quit));
        }

        match self.overlay {
            Some(OverviewOverlay::Help) => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h')
                ) {
                    self.overlay = None;
                }
                return Ok(None);
            }
            Some(OverviewOverlay::SessionMetadata) => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i')
                ) {
                    self.overlay = None;
                }
                return Ok(None);
            }
            Some(OverviewOverlay::SessionEditor(_)) => {
                return self.handle_session_editor_key(database, key);
            }
            Some(OverviewOverlay::GeneralNotesEditor) => {
                return self.handle_notes_editor_key(database, key);
            }
            Some(OverviewOverlay::DeleteSession { .. }) => {
                return self.handle_delete_key(database, key);
            }
            None => {}
        }

        let exit = match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(OverviewExit::Quit),
            KeyCode::Char('h') => {
                self.overlay = Some(OverviewOverlay::Help);
                None
            }
            KeyCode::Char('n') => {
                self.open_session_editor();
                None
            }
            KeyCode::Char('m') => {
                self.open_general_notes_editor();
                None
            }
            KeyCode::Char('e') | KeyCode::Char('t') => {
                self.open_session_metadata_editor();
                None
            }
            KeyCode::Char('i') => {
                self.open_session_metadata();
                None
            }
            KeyCode::Char('D') => {
                self.open_delete_session();
                None
            }
            _ if matches!(key.code, KeyCode::Char('p'))
                || key_matches_binding(&key, &self.config.keys.pomodoro) =>
            {
                self.handle_pomodoro(database, PomodoroKind::Focus)?;
                None
            }
            KeyCode::Char('b') => {
                self.handle_pomodoro(database, PomodoroKind::ShortBreak)?;
                None
            }
            KeyCode::Char('B') => {
                self.handle_pomodoro(database, PomodoroKind::LongBreak)?;
                None
            }
            KeyCode::Char('c') => {
                self.cancel_active_pomodoro(database)?;
                None
            }
            KeyCode::Up | KeyCode::Char('k')
                if matches!(key.code, KeyCode::Up)
                    || key_matches_binding(&key, &self.config.keys.up) =>
            {
                self.move_selection(-1);
                None
            }
            KeyCode::Down | KeyCode::Char('j')
                if matches!(key.code, KeyCode::Down)
                    || key_matches_binding(&key, &self.config.keys.down) =>
            {
                self.move_selection(1);
                None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_index = 0;
                self.scroll_offset = 0;
                None
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_index = self.sessions.len().saturating_sub(1);
                self.ensure_selection_visible(self.visible_rows());
                None
            }
            KeyCode::PageUp => {
                self.move_selection(-(self.visible_rows() as isize));
                None
            }
            KeyCode::PageDown => {
                self.move_selection(self.visible_rows() as isize);
                None
            }
            KeyCode::Enter => {
                self.toggle_selected_session_todos(database)?;
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected_session_name().map(OverviewExit::OpenSession)
            }
            _ => None,
        };

        Ok(exit)
    }

    fn handle_pomodoro(&mut self, database: &mut Database, kind: PomodoroKind) -> Result<()> {
        if let Some(run) = self.active_run.clone() {
            match run.state {
                PomodoroState::Running if matches!(kind, PomodoroKind::Focus) => {
                    database.pause_pomodoro(run.id, now_utc_timestamp())?;
                }
                PomodoroState::Paused if matches!(kind, PomodoroKind::Focus) => {
                    database.resume_pomodoro(run.id, now_utc_timestamp())?;
                }
                _ => {}
            }
        } else {
            database.start_pomodoro(
                kind,
                pomodoro_seconds(&self.config, kind),
                now_utc_timestamp(),
            )?;
        }
        self.reload(database)
    }

    fn cancel_active_pomodoro(&mut self, database: &mut Database) -> Result<()> {
        if let Some(run) = self.active_run.clone() {
            database.cancel_pomodoro(run.id, now_utc_timestamp())?;
            self.reload(database)?;
        }
        Ok(())
    }

    fn handle_session_editor_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<OverviewExit>> {
        match key.code {
            KeyCode::Esc => {
                self.close_session_editor();
                Ok(None)
            }
            KeyCode::Tab => {
                self.session_editor.focused_field = match self.overlay {
                    Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create))
                    | Some(OverviewOverlay::SessionEditor(SessionEditorMode::EditMetadata {
                        ..
                    })) => match self.session_editor.focused_field {
                        EditorField::Primary => EditorField::Secondary,
                        EditorField::Secondary => EditorField::Tertiary,
                        EditorField::Tertiary => EditorField::Primary,
                    },
                    _ => self.session_editor.focused_field,
                };
                Ok(None)
            }
            KeyCode::Enter => self.submit_session_editor(database),
            KeyCode::Backspace => {
                self.current_editor_field_mut().pop();
                self.session_editor.error = None;
                Ok(None)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.current_editor_field_mut().push(character);
                self.session_editor.error = None;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_delete_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<OverviewExit>> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
                Ok(None)
            }
            KeyCode::Enter => {
                let name = match &self.overlay {
                    Some(OverviewOverlay::DeleteSession { name }) => name.clone(),
                    _ => return Ok(None),
                };
                database.delete_session(&name)?;
                self.reload(database)?;
                self.overlay = None;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_notes_editor_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<OverviewExit>> {
        match key.code {
            KeyCode::Esc => {
                self.close_general_notes_editor();
                Ok(None)
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.notes_editor.text.push('\n');
                Ok(None)
            }
            KeyCode::Enter => {
                database.save_overview_notes(self.notes_editor.text.trim_end_matches('\n'))?;
                self.reload(database)?;
                self.close_general_notes_editor();
                Ok(None)
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                database.save_overview_notes(self.notes_editor.text.trim_end_matches('\n'))?;
                self.reload(database)?;
                self.close_general_notes_editor();
                Ok(None)
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.notes_editor.text.push('\n');
                Ok(None)
            }
            KeyCode::Backspace => {
                self.notes_editor.text.pop();
                Ok(None)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.notes_editor.text.push(character);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<OverviewExit> {
        if self.overlay.is_some() {
            return None;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::ScrollDown => self.move_selection(1),
            MouseEventKind::Down(MouseButton::Left) => {
                let list_area = self.list_area(self.last_area);
                if let Some(index) = list_row_index(list_area, self.scroll_offset, mouse.row)
                    && let Some(OverviewDisplayRow::Session(session_index)) =
                        self.display_rows().get(index).copied()
                {
                    self.selected_index = session_index;
                    self.ensure_selection_visible(self.visible_rows());
                }
            }
            _ => {}
        }
        None
    }

    fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        self.last_area = frame.area();
        let chunks = self.root_chunks(frame.area());
        frame.render_widget(Block::default().style(self.theme.app_style()), frame.area());

        frame.render_widget(self.top_bar(), chunks[0]);
        let body_areas = self.body_areas(chunks[1]);

        if self.sessions.is_empty() {
            frame.render_widget(self.empty_state(), body_areas.list);
            frame.render_widget(self.notes_panel(body_areas.notes), body_areas.notes);
            frame.render_widget(self.summary_panel(), body_areas.summary);
        } else {
            frame.render_widget(self.session_list(body_areas.list), body_areas.list);
            frame.render_widget(self.notes_panel(body_areas.notes), body_areas.notes);
            frame.render_widget(self.summary_panel(), body_areas.summary);
            if let Some(details_area) = body_areas.details {
                frame.render_widget(self.details_panel(), details_area);
            }
        }

        if let Some(run) = self.active_run.as_ref()
            && let Some(footer_area) = chunks.get(2).copied()
        {
            frame.render_widget(
                active_footer(&self.theme, run, now_utc_timestamp()),
                footer_area,
            );
        }

        if matches!(self.overlay, Some(OverviewOverlay::Help)) {
            let area = centered_rect(frame.area(), 58, 11);
            frame.render_widget(Clear, area);
            frame.render_widget(self.help_overlay(), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::SessionMetadata)) {
            let area = centered_rect(frame.area(), 60, 13);
            frame.render_widget(Clear, area);
            frame.render_widget(self.session_metadata_modal(), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::SessionEditor(_))) {
            let area = centered_rect(frame.area(), 60, 11);
            frame.render_widget(Clear, area);
            frame.render_widget(self.session_editor_modal(), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::GeneralNotesEditor)) {
            let area = centered_rect(frame.area(), NOTES_EDITOR_WIDTH, NOTES_EDITOR_HEIGHT);
            frame.render_widget(Clear, area);
            frame.render_widget(self.general_notes_editor_modal(), area);
        }
        if let Some(OverviewOverlay::DeleteSession { name }) = &self.overlay {
            let area = centered_rect(frame.area(), 58, 9);
            frame.render_widget(Clear, area);
            frame.render_widget(self.delete_session_modal(name), area);
        }
    }

    fn top_bar(&self) -> Paragraph<'static> {
        let subtitle = if self.sessions.is_empty() {
            if self.has_any_sessions {
                String::from("No sessions with open todos")
            } else {
                String::from("No sessions yet")
            }
        } else {
            format!(
                "{} visible sessions | tag first, then last opened",
                self.sessions.len()
            )
        };

        Paragraph::new(vec![
            Line::from("todui | session overview | h = help"),
            Line::from(subtitle),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Overview")
                .style(self.theme.surface_style(SurfaceTone::Neutral))
                .border_style(self.theme.surface_border_style(SurfaceTone::Open))
                .title_style(self.theme.surface_title_style(SurfaceTone::Open)),
        )
        .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn session_list(&self, area: Rect) -> Paragraph<'static> {
        let visible_rows = self.visible_rows_for_height(area.height);
        let inner_width = usize::from(area.width.saturating_sub(2));
        let mut lines = vec![session_header_line(&self.theme, inner_width)];
        lines.extend(
            self.display_rows()
                .into_iter()
                .skip(self.scroll_offset)
                .take(visible_rows)
                .map(|row| self.display_row_line(row, inner_width)),
        );

        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Sessions")
                    .style(self.theme.surface_style(SurfaceTone::Neutral))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Open))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Open)),
            )
            .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn details_panel(&self) -> Paragraph<'static> {
        let text = self
            .selected_session_metadata_text()
            .unwrap_or_else(|| String::from("Select a session to inspect its summary."));

        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Details")
                    .style(self.theme.surface_style(SurfaceTone::Neutral))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Details))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Details)),
            )
            .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn notes_panel(&self, area: Rect) -> Paragraph<'static> {
        let content = if self.overview_notes.trim().is_empty() {
            ratatui::text::Text::from(vec![
                Line::from("No overview notes yet."),
                Line::from(String::new()),
                Line::from("Press m to edit overview notes."),
            ])
        } else {
            render_markdown(
                &self.theme,
                &self.overview_notes,
                area.width.saturating_sub(2),
            )
        };

        Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("General Notes")
                    .style(self.theme.surface_style(SurfaceTone::Neutral))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Details))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Details)),
            )
            .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn summary_panel(&self) -> Paragraph<'static> {
        let stats = self.summary_stats();
        let text = format!(
            "total sessions: {} | tagged: {} | untagged: {} | avg revision: r{}\ntotal todos: {} | open: {} | completed: {} | completion rate: {}%\nnewest opened: {} | oldest opened: {}",
            stats.total_sessions,
            stats.tagged_sessions,
            stats.untagged_sessions,
            stats.average_revision,
            stats.total_todos,
            stats.open_todos,
            stats.done_todos,
            stats.completion_rate,
            format_month_day_local(stats.newest_last_opened_at),
            format_month_day_local(stats.oldest_last_opened_at)
        );

        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Summary")
                    .style(self.theme.surface_style(SurfaceTone::Neutral))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Open))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Open)),
            )
            .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn empty_state(&self) -> Paragraph<'static> {
        let message = if self.has_any_sessions {
            "No sessions with open todos.\n\nPress n to create one from the TUI."
        } else {
            "No sessions yet.\n\nPress n to create one from the TUI."
        };

        Paragraph::new(message)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Sessions")
                    .style(self.theme.surface_style(SurfaceTone::Neutral))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Open))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Open)),
            )
            .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn help_overlay(&self) -> Paragraph<'static> {
        Paragraph::new(
            "Navigation: j/k, arrows, PageUp/PageDown\nExpand todos: Enter\nOpen session: Right, l\nNew session: n\nEdit notes: m\nEdit session: e\nEdit session alias: t\nSession metadata: i\nDelete session: D\nPomodoro: p start/pause/resume focus\nBreaks: b short break, B long break\nCancel timer: c\nQuit: q or Esc\nClose help: h, q, or Esc",
        )
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help")
                .style(self.theme.surface_style(SurfaceTone::Overlay))
                .border_style(self.theme.surface_border_style(SurfaceTone::Overlay))
                .title_style(self.theme.surface_title_style(SurfaceTone::Overlay)),
        )
        .style(self.theme.surface_style(SurfaceTone::Overlay))
    }

    fn session_editor_modal(&self) -> Paragraph<'_> {
        let (
            title,
            primary_label,
            primary_value,
            secondary_label,
            secondary_value,
            tertiary_label,
            tertiary_value,
            footer_hint,
        ) = match &self.overlay {
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create)) => (
                "New Session",
                "Name",
                self.session_editor.name.as_str(),
                Some("Tag"),
                Some(self.session_editor.tag.as_str()),
                Some("Repo"),
                Some(self.session_editor.repo.as_str()),
                "Tab switch  Enter create  Esc cancel",
            ),
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::EditMetadata { .. })) => (
                "Edit Session Metadata",
                "Name",
                self.session_editor.name.as_str(),
                Some("Tag"),
                Some(self.session_editor.tag.as_str()),
                Some("Repo"),
                Some(self.session_editor.repo.as_str()),
                "Empty clears  Enter save  Esc cancel",
            ),
            _ => ("Session", "Value", "", None, None, None, None, "Esc cancel"),
        };
        render_editor(
            &self.theme,
            EditorView {
                title,
                primary_label,
                primary_value,
                secondary_label,
                secondary_value,
                tertiary_label,
                tertiary_value,
                focused_field: self.session_editor.focused_field,
                error: self.session_editor.error.as_deref(),
                footer_hint,
            },
        )
    }

    fn session_metadata_modal(&self) -> Paragraph<'static> {
        let text = self.selected_session_metadata_text().unwrap_or_else(|| {
            String::from("Select a session to inspect its summary.\n\nEsc close")
        });

        Paragraph::new(format!("{text}\n\nEsc close"))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Session Metadata")
                    .style(self.theme.surface_style(SurfaceTone::Overlay))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Overlay))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Overlay)),
            )
            .style(self.theme.surface_style(SurfaceTone::Overlay))
    }

    fn general_notes_editor_modal(&self) -> Paragraph<'static> {
        let mut body = self.notes_editor.text.clone();
        body.push('|');
        body.push_str("\n\nEnter save  Shift+Enter newline  Ctrl+J newline  Esc cancel");

        Paragraph::new(body)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Edit General Notes")
                    .style(self.theme.surface_style(SurfaceTone::Overlay))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Overlay))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Overlay)),
            )
            .style(self.theme.surface_style(SurfaceTone::Overlay))
    }

    fn delete_session_modal(&self, name: &str) -> Paragraph<'static> {
        Paragraph::new(format!(
            "Delete session {name}?\n\nThis permanently removes its todos and history.\n\nEnter delete  Esc cancel"
        ))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Delete Session")
                .style(self.theme.surface_style(SurfaceTone::Danger))
                .border_style(self.theme.surface_border_style(SurfaceTone::Danger))
                .title_style(self.theme.surface_title_style(SurfaceTone::Danger)),
        )
        .style(self.theme.surface_style(SurfaceTone::Danger))
    }

    fn selected_session_name(&self) -> Option<String> {
        self.sessions
            .get(self.selected_index)
            .map(|session| session.name.clone())
    }

    fn visible_rows(&self) -> usize {
        self.visible_rows_for_height(self.list_area(self.last_area).height)
    }

    fn visible_rows_for_height(&self, height: u16) -> usize {
        height.saturating_sub(3).max(1) as usize
    }

    fn move_selection(&mut self, delta: isize) {
        if self.sessions.is_empty() {
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }

        if delta.is_negative() {
            self.selected_index = self.selected_index.saturating_sub(delta.unsigned_abs());
        } else {
            self.selected_index = min(
                self.selected_index + delta as usize,
                self.sessions.len().saturating_sub(1),
            );
        }
        self.ensure_selection_visible(self.visible_rows());
    }

    fn ensure_selection_visible(&mut self, visible_rows: usize) {
        let Some(selected_display_index) = self.selected_display_index() else {
            self.scroll_offset = 0;
            return;
        };

        if selected_display_index < self.scroll_offset {
            self.scroll_offset = selected_display_index;
        } else if selected_display_index >= self.scroll_offset + visible_rows {
            self.scroll_offset = selected_display_index + 1 - visible_rows;
        }

        let expanded_child_rows = self.expanded_child_row_count(self.selected_index);
        if expanded_child_rows > 0 && expanded_child_rows < visible_rows {
            let expanded_end = selected_display_index + expanded_child_rows;
            let min_offset = expanded_end.saturating_add(1).saturating_sub(visible_rows);
            if self.scroll_offset < min_offset {
                self.scroll_offset = min(min_offset, selected_display_index);
            }
        }

        let max_scroll = self.display_rows().len().saturating_sub(visible_rows);
        self.scroll_offset = min(self.scroll_offset, max_scroll);
    }

    fn list_area(&self, area: Rect) -> Rect {
        self.body_areas(self.root_chunks(area)[1]).list
    }

    fn body_areas(&self, body: Rect) -> OverviewBodyAreas {
        if body.width >= 90 {
            let columns =
                Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                    .split(body);
            let left_column = Layout::vertical([
                Constraint::Percentage(OVERVIEW_LIST_PERCENT),
                Constraint::Percentage(OVERVIEW_NOTES_PERCENT),
                Constraint::Percentage(OVERVIEW_SUMMARY_PERCENT),
            ])
            .split(columns[0]);
            OverviewBodyAreas {
                list: left_column[0],
                notes: left_column[1],
                summary: left_column[2],
                details: Some(columns[1]),
            }
        } else {
            let stacked = Layout::vertical([
                Constraint::Percentage(OVERVIEW_LIST_PERCENT),
                Constraint::Percentage(OVERVIEW_NOTES_PERCENT),
                Constraint::Percentage(OVERVIEW_SUMMARY_PERCENT),
            ])
            .split(body);
            OverviewBodyAreas {
                list: stacked[0],
                notes: stacked[1],
                summary: stacked[2],
                details: None,
            }
        }
    }

    fn root_chunks(&self, area: Rect) -> Vec<Rect> {
        let mut constraints = vec![Constraint::Length(3), Constraint::Min(8)];
        if self.active_run.is_some() {
            constraints.push(Constraint::Length(active_footer_height()));
        }
        Layout::vertical(constraints).split(area).to_vec()
    }

    fn summary_stats(&self) -> OverviewSummaryStats {
        let total_sessions = self.sessions.len();
        let tagged_sessions = self
            .sessions
            .iter()
            .filter(|session| session.tag.is_some())
            .count();
        let total_todos = self
            .sessions
            .iter()
            .map(|session| session.todo_count)
            .sum::<i64>();
        let done_todos = self
            .sessions
            .iter()
            .map(|session| session.done_count)
            .sum::<i64>();
        let open_todos = total_todos - done_todos;
        let completion_rate = if total_todos == 0 {
            0
        } else {
            ((done_todos * 100) + (total_todos / 2)) / total_todos
        };
        let newest_last_opened_at = self
            .sessions
            .iter()
            .map(|session| session.last_opened_at)
            .max()
            .unwrap_or(0);
        let oldest_last_opened_at = self
            .sessions
            .iter()
            .map(|session| session.last_opened_at)
            .min()
            .unwrap_or(0);
        let total_revisions = self
            .sessions
            .iter()
            .map(|session| u64::from(session.current_revision))
            .sum::<u64>();
        let average_revision = if total_sessions == 0 {
            0
        } else {
            ((total_revisions + (total_sessions as u64 / 2)) / total_sessions as u64) as u32
        };

        OverviewSummaryStats {
            total_sessions,
            tagged_sessions,
            untagged_sessions: total_sessions.saturating_sub(tagged_sessions),
            total_todos,
            open_todos,
            done_todos,
            completion_rate,
            newest_last_opened_at,
            oldest_last_opened_at,
            average_revision,
        }
    }

    fn open_session_editor(&mut self) {
        self.overlay = Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create));
        self.session_editor = SessionEditorState::default();
    }

    fn open_session_metadata_editor(&mut self) {
        let Some(session) = self.sessions.get(self.selected_index) else {
            return;
        };
        self.overlay = Some(OverviewOverlay::SessionEditor(
            SessionEditorMode::EditMetadata {
                name: session.name.clone(),
            },
        ));
        self.session_editor = SessionEditorState {
            name: session.name.clone(),
            tag: session.tag.clone().unwrap_or_default(),
            repo: session.repo.clone().unwrap_or_default(),
            focused_field: EditorField::Primary,
            error: None,
        };
    }

    fn open_session_metadata(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        self.overlay = Some(OverviewOverlay::SessionMetadata);
    }

    fn open_general_notes_editor(&mut self) {
        self.overlay = Some(OverviewOverlay::GeneralNotesEditor);
        self.notes_editor = GeneralNotesEditorState {
            text: self.overview_notes.clone(),
        };
    }

    fn open_delete_session(&mut self) {
        let Some(session) = self.sessions.get(self.selected_index) else {
            return;
        };
        self.overlay = Some(OverviewOverlay::DeleteSession {
            name: session.name.clone(),
        });
    }

    fn close_session_editor(&mut self) {
        self.overlay = None;
        self.session_editor = SessionEditorState::default();
    }

    fn close_general_notes_editor(&mut self) {
        self.overlay = None;
        self.notes_editor = GeneralNotesEditorState::default();
    }

    fn toggle_selected_session_todos(&mut self, database: &Database) -> Result<()> {
        let Some(session_name) = self.selected_session_name() else {
            return Ok(());
        };

        if self
            .expanded_sessions
            .iter()
            .any(|expanded| expanded.name == session_name)
        {
            self.expanded_sessions
                .retain(|expanded| expanded.name != session_name);
            self.ensure_selection_visible(self.visible_rows());
            return Ok(());
        }

        let session = database.get_session_by_name(&session_name)?;
        self.expanded_sessions.push(ExpandedSessionState {
            name: session_name,
            todos: open_preview_todos(database.get_live_todos(session.id)?),
        });
        self.ensure_selection_visible(self.visible_rows());
        Ok(())
    }

    fn submit_session_editor(&mut self, database: &mut Database) -> Result<Option<OverviewExit>> {
        match &self.overlay {
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create)) => {
                let name = self.session_editor.name.trim();
                if name.is_empty() {
                    self.session_editor.error = Some(String::from("Session name is required"));
                    return Ok(None);
                }

                match database.create_session(
                    name,
                    Some(self.session_editor.tag.as_str()),
                    Some(self.session_editor.repo.as_str()),
                    now_utc_timestamp(),
                ) {
                    Ok(session) => {
                        self.reload(database)?;
                        self.close_session_editor();
                        Ok(Some(OverviewExit::OpenSession(session.name)))
                    }
                    Err(error) => {
                        self.session_editor.error = Some(error.to_string());
                        Ok(None)
                    }
                }
            }
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::EditMetadata {
                name: current_name,
            })) => {
                let next_name = self.session_editor.name.trim();
                if next_name.is_empty() {
                    self.session_editor.error = Some(String::from("Session name is required"));
                    return Ok(None);
                }

                match database.edit_session(
                    current_name,
                    next_name,
                    Some(self.session_editor.tag.as_str()),
                    Some(self.session_editor.repo.as_str()),
                    now_utc_timestamp(),
                ) {
                    Ok(session) => {
                        for expanded in &mut self.expanded_sessions {
                            if expanded.name == *current_name {
                                expanded.name = session.name.clone();
                            }
                        }
                        self.reload(database)?;
                        self.close_session_editor();
                        Ok(None)
                    }
                    Err(error) => {
                        self.session_editor.error = Some(error.to_string());
                        Ok(None)
                    }
                }
            }
            _ => Ok(None),
        }
    }

    fn current_editor_field_mut(&mut self) -> &mut String {
        match self.session_editor.focused_field {
            EditorField::Primary => &mut self.session_editor.name,
            EditorField::Secondary => &mut self.session_editor.tag,
            EditorField::Tertiary => &mut self.session_editor.repo,
        }
    }

    fn sync_expanded_sessions(&mut self, database: &Database) -> Result<()> {
        let mut refreshed_sessions = Vec::with_capacity(self.expanded_sessions.len());
        for expanded in &self.expanded_sessions {
            if self
                .sessions
                .iter()
                .all(|session| session.name != expanded.name)
            {
                continue;
            }

            let session = database.get_session_by_name(&expanded.name)?;
            refreshed_sessions.push(ExpandedSessionState {
                name: expanded.name.clone(),
                todos: open_preview_todos(database.get_live_todos(session.id)?),
            });
        }
        self.expanded_sessions = refreshed_sessions;
        Ok(())
    }

    fn display_rows(&self) -> Vec<OverviewDisplayRow> {
        let mut rows = Vec::with_capacity(self.sessions.len() + self.expanded_row_count());
        for (session_index, session) in self.sessions.iter().enumerate() {
            rows.push(OverviewDisplayRow::Session(session_index));
            if let Some(todos) = self.expanded_todos_for_session(&session.name) {
                if todos.is_empty() {
                    rows.push(OverviewDisplayRow::EmptyTodos(session_index));
                } else {
                    rows.extend((0..todos.len()).map(|todo_index| OverviewDisplayRow::Todo {
                        session_index,
                        todo_index,
                    }));
                }
            }
        }
        rows
    }

    fn display_row_line(&self, row: OverviewDisplayRow, inner_width: usize) -> Line<'static> {
        match row {
            OverviewDisplayRow::Session(session_index) => session_row_line(
                &self.sessions[session_index],
                &self.theme,
                inner_width,
                session_index == self.selected_index,
            ),
            OverviewDisplayRow::Todo {
                session_index,
                todo_index,
            } => todo_preview_line(
                &self
                    .expanded_todos_for_session(&self.sessions[session_index].name)
                    .expect("expanded todos for row")[todo_index],
                &self.theme,
                inner_width,
            ),
            OverviewDisplayRow::EmptyTodos(_) => empty_todo_preview_line(&self.theme, inner_width),
        }
    }

    fn selected_display_index(&self) -> Option<usize> {
        self.display_rows().iter().position(|row| {
            matches!(row, OverviewDisplayRow::Session(session_index) if *session_index == self.selected_index)
        })
    }

    fn expanded_todos_for_session(&self, session_name: &str) -> Option<&[Todo]> {
        self.expanded_sessions
            .iter()
            .find(|expanded| expanded.name == session_name)
            .map(|expanded| expanded.todos.as_slice())
    }

    fn expanded_child_row_count(&self, session_index: usize) -> usize {
        self.sessions
            .get(session_index)
            .and_then(|session| self.expanded_todos_for_session(&session.name))
            .map(|todos| todos.len().max(1))
            .unwrap_or(0)
    }

    fn expanded_row_count(&self) -> usize {
        self.expanded_sessions
            .iter()
            .map(|expanded| expanded.todos.len().max(1))
            .sum()
    }

    fn selected_session_metadata_text(&self) -> Option<String> {
        self.sessions.get(self.selected_index).map(|session| {
            format!(
                "session: {}\ntag: {}\nrepo: {}\nlast opened: {}\ncurrent revision: r{}\nopen todos: {}\ndone todos: {}\n\nEnter expands the session todos.\nUse Right or l to open the session head.\nUse o inside the session to return here.\nUse H inside the session for revision history.",
                session.name,
                session.tag.as_deref().unwrap_or("untagged"),
                session.repo.as_deref().unwrap_or("-"),
                format_full_local(session.last_opened_at),
                session.current_revision,
                session.todo_count - session.done_count,
                session.done_count
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OverviewSummaryStats {
    total_sessions: usize,
    tagged_sessions: usize,
    untagged_sessions: usize,
    total_todos: i64,
    open_todos: i64,
    done_todos: i64,
    completion_rate: i64,
    newest_last_opened_at: i64,
    oldest_last_opened_at: i64,
    average_revision: u32,
}

#[derive(Debug, Clone, Copy)]
struct OverviewBodyAreas {
    list: Rect,
    notes: Rect,
    summary: Rect,
    details: Option<Rect>,
}

fn session_header_line(theme: &Theme, inner_width: usize) -> Line<'static> {
    let widths = session_column_widths(inner_width);
    Line::from(vec![
        Span::styled(
            fit_cell("Tag", widths.tag),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("Name", widths.name),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("Rev", widths.rev),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("☐", widths.open),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("☑", widths.done),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("Last Opened", widths.last_opened),
            theme.surface_title_style(SurfaceTone::Open),
        ),
    ])
}

fn session_row_line(
    session: &SessionOverview,
    theme: &Theme,
    inner_width: usize,
    is_selected: bool,
) -> Line<'static> {
    let widths = session_column_widths(inner_width);
    let mut line = Line::from(vec![
        Span::styled(
            fit_cell(session.tag.as_deref().unwrap_or("-"), widths.tag),
            if session.tag.is_some() {
                theme.text_style(TextTone::Tag)
            } else {
                theme.text_style(TextTone::Muted)
            },
        ),
        Span::raw(" "),
        Span::raw(fit_cell(&session.name, widths.name)),
        Span::raw(" "),
        Span::styled(
            fit_cell(&format!("r{}", session.current_revision), widths.rev),
            theme.text_style(TextTone::Meta),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell(
                &(session.todo_count - session.done_count).to_string(),
                widths.open,
            ),
            theme.text_style(TextTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell(&session.done_count.to_string(), widths.done),
            theme.text_style(TextTone::Completed),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell(
                &format_month_day_local(session.last_opened_at),
                widths.last_opened,
            ),
            theme.text_style(TextTone::Muted),
        ),
    ]);
    if is_selected {
        line = line.style(theme.selection_style(SelectionTone::Open));
    }
    line
}

fn todo_preview_line(todo: &Todo, theme: &Theme, inner_width: usize) -> Line<'static> {
    let title_width = inner_width
        .saturating_sub(TODO_PREVIEW_TIME_WIDTH + 1)
        .max(1);
    let time_width = inner_width.saturating_sub(title_width);
    let title = fit_cell(&format!("  [ ] {}", todo.title), title_width);
    let time = right_align_cell(&format_month_day_local(todo.created_at), time_width);

    Line::from(vec![
        Span::styled(title, theme.text_style(TextTone::Focus)),
        Span::styled(time, theme.text_style(TextTone::Muted)),
    ])
}

fn empty_todo_preview_line(theme: &Theme, inner_width: usize) -> Line<'static> {
    Line::from(Span::styled(
        fit_cell("  No open todos", inner_width),
        theme.text_style(TextTone::Muted),
    ))
}

fn session_column_widths(inner_width: usize) -> SessionColumnWidths {
    let name_width = inner_width.saturating_sub(
        TAG_COLUMN_WIDTH
            + REV_COLUMN_WIDTH
            + OPEN_COLUMN_WIDTH
            + DONE_COLUMN_WIDTH
            + LAST_OPENED_COLUMN_WIDTH
            + SESSION_COLUMN_SPACING,
    );
    SessionColumnWidths {
        tag: TAG_COLUMN_WIDTH,
        name: name_width.max(1),
        rev: REV_COLUMN_WIDTH,
        open: OPEN_COLUMN_WIDTH,
        done: DONE_COLUMN_WIDTH,
        last_opened: LAST_OPENED_COLUMN_WIDTH,
    }
}

fn fit_cell(text: &str, width: usize) -> String {
    let mut value = text.chars().take(width).collect::<String>();
    let padding = width.saturating_sub(value.chars().count());
    value.push_str(&" ".repeat(padding));
    value
}

fn right_align_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let value = text.chars().take(width).collect::<String>();
    let padding = width.saturating_sub(value.chars().count());
    format!("{}{}", " ".repeat(padding), value)
}

fn open_preview_todos(todos: Vec<Todo>) -> Vec<Todo> {
    todos
        .into_iter()
        .filter(|todo| matches!(todo.status, TodoStatus::Open))
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SessionColumnWidths {
    tag: usize,
    name: usize,
    rev: usize,
    open: usize,
    done: usize,
    last_opened: usize,
}

fn pomodoro_seconds(config: &Config, kind: PomodoroKind) -> i64 {
    match kind {
        PomodoroKind::Focus => i64::from(config.pomodoro.focus_minutes) * 60,
        PomodoroKind::ShortBreak => i64::from(config.pomodoro.short_break_minutes) * 60,
        PomodoroKind::LongBreak => i64::from(config.pomodoro.long_break_minutes) * 60,
    }
}

fn key_matches_binding(key: &KeyEvent, bindings: &[String]) -> bool {
    bindings.iter().any(|binding| match binding.as_str() {
        "up" => matches!(key.code, KeyCode::Up),
        "down" => matches!(key.code, KeyCode::Down),
        value if value.len() == 1 => {
            matches!(key.code, KeyCode::Char(character) if value.starts_with(character))
        }
        _ => false,
    })
}

fn list_row_index(list_area: Rect, scroll_offset: usize, y: u16) -> Option<usize> {
    let inner_y = list_area.y.saturating_add(1);
    if y <= inner_y || y >= list_area.bottom().saturating_sub(1) {
        return None;
    }

    Some(scroll_offset + usize::from(y.saturating_sub(inner_y + 1)))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use super::{OverviewExit, OverviewScreen, list_row_index, todo_preview_line};
    use crate::config::Config;
    use crate::db::Database;
    use crate::domain::pomodoro::PomodoroKind;
    use crate::domain::todo::Todo;
    use crate::domain::todo::TodoStatus;
    use crate::timestamp::format_month_day_local;
    use crate::tui::theme::{TextTone, Theme};

    #[test]
    fn overview_screen_handles_navigation_and_open() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("reading-sprint")
        );

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("writing-sprint")
        );

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Right))
            .unwrap();
        assert_eq!(
            exit,
            Some(OverviewExit::OpenSession(String::from("writing-sprint")))
        );

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(exit.is_none());
        let expanded = render_buffer(&mut screen, 120, 24);
        assert!(expanded.contains("[ ] Draft spec"));

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(exit.is_none());
        let collapsed = render_buffer(&mut screen, 120, 24);
        assert!(!collapsed.contains("[ ] Draft spec"));
    }

    #[test]
    fn overview_enter_toggles_inline_todos_for_multiple_sessions() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("expand");
        let expanded = render_buffer(&mut screen, 120, 24);
        assert!(expanded.contains("No open todos"));

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move");
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("writing-sprint")
        );

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("expand second");
        let expanded = render_buffer(&mut screen, 120, 24);
        assert!(expanded.contains("[ ] Draft spec"));
        assert!(expanded.contains("No open todos"));

        screen
            .handle_key(&mut database, key(KeyCode::Up))
            .expect("move back");
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("collapse first");
        let partially_collapsed = render_buffer(&mut screen, 120, 24);
        assert!(partially_collapsed.contains("[ ] Draft spec"));
        assert!(!partially_collapsed.contains("No open todos"));
    }

    #[test]
    fn overview_expanded_todo_preview_shows_only_open_todos_with_right_aligned_time() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);
        let long_title =
            String::from("very-long-inline-preview-title-abcdefghijklmnopqrstuvwxyz-1234567890");
        let long_todo = database
            .add_todo("writing-sprint", &long_title, "", None, 1_711_275_900)
            .expect("long todo");
        let done_todo = database
            .add_todo(
                "writing-sprint",
                "done todo should stay hidden",
                "",
                None,
                1_711_275_930,
            )
            .expect("done todo");
        database
            .set_todo_status(
                done_todo.id,
                Some("writing-sprint"),
                TodoStatus::Done,
                1_711_275_960,
            )
            .expect("mark done");

        screen.reload(&database).expect("reload");
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move");
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("expand");

        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("very-long-inline-preview-title"));
        assert!(buffer.contains(&format_month_day_local(long_todo.created_at)));
        assert!(!buffer.contains("done todo should stay hidden"));
    }

    #[test]
    fn overview_expanded_todo_preview_uses_distinct_focus_tone() {
        let theme = Theme::default();
        let line = todo_preview_line(
            &Todo {
                id: 1,
                session_id: 1,
                title: String::from("Draft spec"),
                notes: String::new(),
                repo: None,
                status: TodoStatus::Open,
                position: 1,
                created_at: 1_711_275_900,
                updated_at: 1_711_275_900,
                completed_at: None,
            },
            &theme,
            50,
        );

        assert_eq!(line.spans[0].style.fg, theme.text_style(TextTone::Focus).fg);
        assert_eq!(line.spans[1].style.fg, theme.text_style(TextTone::Muted).fg);
    }

    #[test]
    fn overview_creates_session_from_modal_and_opens_it() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        assert!(
            screen
                .handle_key(&mut database, key(KeyCode::Char('n')))
                .unwrap()
                .is_none()
        );
        assert!(render_buffer(&mut screen, 120, 24).contains("New Session"));

        for character in "Inbox".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for character in "Private".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(exit, Some(OverviewExit::OpenSession(String::from("inbox"))));
        assert_eq!(
            database
                .get_session_by_name("inbox")
                .expect("new session")
                .tag
                .as_deref(),
            Some("private")
        );
        assert_eq!(
            database
                .get_session_by_name("inbox")
                .expect("new session")
                .name,
            "inbox"
        );
    }

    #[test]
    fn overview_blocks_blank_session_name() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();

        assert!(exit.is_none());
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("Session name is required"));
        assert!(database.get_session_by_name("inbox").is_err());
    }

    #[test]
    fn overview_mouse_selects_rows_without_opening() {
        let (_directory, _database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);
        let list_area = screen.list_area(screen.last_area);
        let row = list_area.y + 3;

        let exit = screen.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 4, row));
        assert!(exit.is_none());
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("writing-sprint")
        );
    }

    #[test]
    fn overview_render_covers_populated_and_empty_states() {
        let (_directory, _database, mut populated) = seeded_overview_screen();
        let wide_buffer = render_buffer(&mut populated, 120, 24);
        assert!(wide_buffer.contains("session overview"));
        assert!(wide_buffer.contains("h = help"));
        assert!(wide_buffer.contains("Tag"));
        assert!(wide_buffer.contains("Session"));
        assert!(wide_buffer.contains("Last Opened"));
        assert!(wide_buffer.contains("private"));
        assert!(wide_buffer.contains("writing-sprint"));
        assert!(wide_buffer.contains("General Notes"));
        assert!(wide_buffer.contains("Press m to edit overview notes."));
        assert!(wide_buffer.contains("Summary"));
        assert!(wide_buffer.contains("total sessions: 2"));
        assert!(wide_buffer.contains("tagged: 2"));
        assert!(wide_buffer.contains("untagged: 0"));
        assert!(wide_buffer.contains("total todos: 1"));
        assert!(wide_buffer.contains("open: 1"));
        assert!(wide_buffer.contains("completed: 0"));
        assert!(wide_buffer.contains("completion rate: 0%"));
        assert!(wide_buffer.contains("Enter expands the session todos."));
        assert!(wide_buffer.contains("Use Right or l to open the session head."));
        assert!(wide_buffer.contains("return here."));
        assert!(!wide_buffer.contains("Pomodoro"));
        assert!(!wide_buffer.contains("Keys"));

        let narrow_buffer = render_buffer(&mut populated, 80, 20);
        assert!(narrow_buffer.contains("Sessions"));
        assert!(narrow_buffer.contains("General Notes"));
        assert!(narrow_buffer.contains("Summary"));
        assert!(!narrow_buffer.contains("Details"));

        let (_directory, database) = Database::open_temp().expect("database");
        let mut empty = OverviewScreen::new(Config::default());
        empty.reload(&database).expect("reload");
        let empty_buffer = render_buffer(&mut empty, 80, 20);
        assert!(empty_buffer.contains("No sessions yet."));
        assert!(empty_buffer.contains("Press n to create one"));
        assert!(empty_buffer.contains("h = help"));
        assert!(!empty_buffer.contains("Pomodoro"));
        assert!(empty_buffer.contains("General Notes"));
        assert!(empty_buffer.contains("Summary"));
        assert!(!empty_buffer.contains("Keys"));
    }

    #[test]
    fn overview_layout_uses_forty_forty_twenty_split() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let body = screen.body_areas(Rect::new(0, 0, 80, 20));

        assert_eq!(body.list.height, body.notes.height);
        assert!(body.summary.height < body.notes.height);
    }

    #[test]
    fn overview_hides_fully_completed_sessions_but_keeps_empty_ones() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let work = database
            .create_session("Writing Sprint", Some("work"), None, 1_711_275_600)
            .expect("session");
        let open_todo = database
            .add_todo(&work.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");
        let completed = database
            .create_session("Finished Sprint", Some("private"), None, 1_711_275_700)
            .expect("session");
        let completed_todo = database
            .add_todo(&completed.name, "Done task", "", None, 1_711_275_710)
            .expect("todo");
        database
            .set_todo_status(
                completed_todo.id,
                Some(&completed.name),
                TodoStatus::Done,
                1_711_275_720,
            )
            .expect("done");
        let empty = database
            .create_session("Inbox", None, None, 1_711_275_900)
            .expect("session");
        database
            .mark_session_opened(&work.name, 1_711_276_000)
            .expect("opened");

        let mut screen = OverviewScreen::new(Config::default());
        screen.reload(&database).expect("reload");

        assert_eq!(
            screen
                .sessions
                .iter()
                .map(|session| session.name.as_str())
                .collect::<Vec<_>>(),
            vec![work.name.as_str(), empty.name.as_str()]
        );
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("writing-sprint")
        );
        assert!(
            screen
                .sessions
                .iter()
                .all(|session| session.name != completed.name)
        );
        assert_eq!(screen.summary_stats().total_sessions, 2);
        assert_eq!(screen.summary_stats().total_todos, 1);
        assert_eq!(screen.summary_stats().open_todos, 1);
        assert_eq!(screen.summary_stats().done_todos, 0);

        database
            .set_todo_status(
                open_todo.id,
                Some(&work.name),
                TodoStatus::Done,
                1_711_276_010,
            )
            .expect("done");
        screen.reload(&database).expect("reload");

        assert_eq!(
            screen
                .sessions
                .iter()
                .map(|session| session.name.as_str())
                .collect::<Vec<_>>(),
            vec![empty.name.as_str()]
        );
        assert_eq!(screen.selected_session_name().as_deref(), Some("inbox"));
    }

    #[test]
    fn overview_empty_state_distinguishes_completed_only_sessions() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let completed = database
            .create_session("Finished Sprint", Some("private"), None, 1_711_275_700)
            .expect("session");
        let completed_todo = database
            .add_todo(&completed.name, "Done task", "", None, 1_711_275_710)
            .expect("todo");
        database
            .set_todo_status(
                completed_todo.id,
                Some(&completed.name),
                TodoStatus::Done,
                1_711_275_720,
            )
            .expect("done");

        let mut screen = OverviewScreen::new(Config::default());
        screen.reload(&database).expect("reload");

        let buffer = render_buffer(&mut screen, 80, 20);
        assert!(buffer.contains("No sessions with open todos."));
        assert!(buffer.contains("Press n to create one"));
        assert!(!buffer.contains("No sessions yet."));
        assert!(buffer.contains("General Notes"));
        assert!(buffer.contains("Summary"));
    }

    #[test]
    fn overview_orders_sessions_by_tag_then_recent_date() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let work = database
            .create_session("Writing Sprint", Some("work"), None, 1_711_275_600)
            .expect("session");
        let private = database
            .create_session("Reading Sprint", Some("private"), None, 1_711_275_700)
            .expect("session");
        let inbox = database
            .create_session("Inbox", None, None, 1_711_275_900)
            .expect("session");
        database
            .mark_session_opened(&work.name, 1_711_276_000)
            .expect("opened");

        let mut screen = OverviewScreen::new(Config::default());
        screen.reload(&database).expect("reload");

        assert_eq!(
            screen
                .sessions
                .iter()
                .map(|session| session.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                private.name.as_str(),
                work.name.as_str(),
                inbox.name.as_str()
            ]
        );
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("reading-sprint")
        );
    }

    #[test]
    fn overview_shows_active_pomodoro_footer() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();

        database
            .start_pomodoro(PomodoroKind::ShortBreak, 300, 1_711_275_900)
            .expect("run");
        screen.reload(&database).expect("reload");

        let rendered = render_buffer(&mut screen, 120, 24);
        assert!(rendered.contains("Pomodoro"));
        assert!(rendered.contains("SHORT BREAK"));
        assert!(!rendered.contains("Linked:"));
        assert!(!rendered.contains("No linked todo"));
    }

    #[test]
    fn overview_handles_pomodoro_shortcuts() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().map(|run| run.kind),
            Some(PomodoroKind::Focus)
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().map(|run| run.state),
            Some(crate::domain::pomodoro::PomodoroState::Paused)
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().map(|run| run.state),
            Some(crate::domain::pomodoro::PomodoroState::Running)
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('c')))
            .unwrap();
        assert!(screen.active_run.is_none());

        screen
            .handle_key(&mut database, key(KeyCode::Char('b')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().map(|run| run.kind),
            Some(PomodoroKind::ShortBreak)
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('c')))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Char('B')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().map(|run| run.kind),
            Some(PomodoroKind::LongBreak)
        ));
    }

    #[test]
    fn overview_honors_custom_pomodoro_keybinding() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", Some("work"), None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&session.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");

        let mut config = Config::default();
        config.keys.pomodoro = vec![String::from("x")];
        let mut screen = OverviewScreen::new(config);
        screen.reload(&database).expect("reload");

        screen
            .handle_key(&mut database, key(KeyCode::Char('x')))
            .unwrap();

        assert!(matches!(
            screen.active_run.as_ref().map(|run| run.kind),
            Some(PomodoroKind::Focus)
        ));
    }

    #[test]
    fn overview_summary_stats_include_activity_mix() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let stats = screen.summary_stats();

        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.tagged_sessions, 2);
        assert_eq!(stats.untagged_sessions, 0);
        assert_eq!(stats.total_todos, 1);
        assert_eq!(stats.open_todos, 1);
        assert_eq!(stats.done_todos, 0);
        assert_eq!(stats.completion_rate, 0);
        assert_eq!(stats.newest_last_opened_at, 1_711_275_800);
        assert_eq!(stats.oldest_last_opened_at, 1_711_275_600);
        assert_eq!(stats.average_revision, 2);
    }

    #[test]
    fn overview_opens_help_overlay_with_h() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('h')))
            .unwrap();
        assert!(render_buffer(&mut screen, 120, 24).contains("Help"));
        assert!(render_buffer(&mut screen, 120, 24).contains("Expand todos"));
        assert!(render_buffer(&mut screen, 120, 24).contains("Open session"));

        screen.handle_key(&mut database, key(KeyCode::Esc)).unwrap();
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(!buffer.contains("Expand todos"));
        assert!(!buffer.contains("Open session"));
    }

    #[test]
    fn overview_confirms_and_deletes_selected_session() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('D')))
            .unwrap();
        assert!(render_buffer(&mut screen, 120, 24).contains("Delete Session"));

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(exit.is_none());
        assert!(database.get_session_by_name("reading-sprint").is_err());
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("writing-sprint")
        );
    }

    #[test]
    fn overview_cancel_keeps_selected_session() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('D')))
            .unwrap();
        screen.handle_key(&mut database, key(KeyCode::Esc)).unwrap();

        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("reading-sprint")
        );
        assert!(database.get_session_by_name("reading-sprint").is_ok());
    }

    #[test]
    fn overview_edits_selected_session_metadata() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);
        let reading_todo = database
            .add_todo("reading-sprint", "Review paper", "", None, 1_711_275_810)
            .expect("todo");

        screen
            .handle_key(&mut database, key(KeyCode::Char('e')))
            .unwrap();
        assert!(render_buffer(&mut screen, 120, 24).contains("Edit Session Metadata"));
        for _ in 0..14 {
            screen
                .handle_key(&mut database, key(KeyCode::Backspace))
                .unwrap();
        }
        for character in "Research Sprint".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for _ in 0..7 {
            screen
                .handle_key(&mut database, key(KeyCode::Backspace))
                .unwrap();
        }
        for character in "Deep Work".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for character in "@ExampleOrg/todui-keymove".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();

        assert_eq!(
            database
                .get_session_by_name("research-sprint")
                .expect("session")
                .name,
            "research-sprint"
        );
        assert_eq!(
            database
                .get_session_by_name("research-sprint")
                .expect("session")
                .tag
                .as_deref(),
            Some("deep-work")
        );
        assert_eq!(
            database
                .get_session_by_name("research-sprint")
                .expect("session")
                .repo
                .as_deref(),
            Some("exampleorg/todui-keymove")
        );
        assert_eq!(
            database
                .get_todo(reading_todo.id)
                .expect("todo")
                .repo
                .as_deref(),
            Some("exampleorg/todui-keymove")
        );
        assert!(database.get_session_by_name("reading-sprint").is_err());
    }

    #[test]
    fn overview_i_opens_and_closes_metadata_popup() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('i')))
            .unwrap();
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("Session Metadata"));
        assert!(buffer.contains("session: reading-sprint"));

        screen
            .handle_key(&mut database, key(KeyCode::Char('i')))
            .unwrap();
        assert!(!render_buffer(&mut screen, 120, 24).contains("Session Metadata"));
    }

    #[test]
    fn overview_edits_general_notes_and_persists_raw_markdown() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('m')))
            .expect("open notes editor");
        assert!(render_buffer(&mut screen, 120, 24).contains("Edit General Notes"));
        for character in "# Focus".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .expect("type heading");
        }
        screen
            .handle_key(&mut database, ctrl_key(KeyCode::Char('j')))
            .expect("newline");
        for character in "Ship **notes**".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .expect("type body");
        }

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("save");
        assert!(exit.is_none());
        assert_eq!(
            database.get_overview_notes().expect("notes"),
            "# Focus\nShip **notes**"
        );

        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("General Notes"));
        assert!(buffer.contains("Focus"));
        assert!(buffer.contains("Ship notes"));
    }

    #[test]
    fn overview_shift_enter_inserts_newline_without_saving() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('m')))
            .expect("open notes editor");
        for character in "# Focus".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .expect("type heading");
        }

        screen
            .handle_key(&mut database, shift_key(KeyCode::Enter))
            .expect("shift enter newline");
        for character in "Line two".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .expect("type body");
        }

        assert_eq!(database.get_overview_notes().expect("notes"), "");
        assert!(render_buffer(&mut screen, 120, 24).contains("Edit General Notes"));

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("save");
        assert_eq!(
            database.get_overview_notes().expect("notes"),
            "# Focus\nLine two"
        );
    }

    #[test]
    fn overview_mouse_click_ignores_inline_todo_rows() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move to writing");
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("expand");

        let list_area = screen.list_area(screen.last_area);
        let todo_row = list_area.y + 3;

        let exit = screen.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 4, todo_row));
        assert!(exit.is_none());
        assert_eq!(
            screen.selected_session_name().as_deref(),
            Some("writing-sprint")
        );
    }

    #[test]
    fn list_row_index_uses_inner_rows_only() {
        let area = Rect::new(0, 0, 40, 10);
        assert_eq!(list_row_index(area, 0, 0), None);
        assert_eq!(list_row_index(area, 0, 1), None);
        assert_eq!(list_row_index(area, 0, 2), Some(0));
        assert_eq!(list_row_index(area, 0, 3), Some(1));
        assert_eq!(list_row_index(area, 2, 4), Some(4));
    }

    fn seeded_overview_screen() -> (tempfile::TempDir, Database, OverviewScreen) {
        let (directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", Some("work"), None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.name, "Draft spec", "", None, 1_711_275_650)
            .expect("todo");
        let reading = database
            .create_session("Reading Sprint", Some("private"), None, 1_711_275_700)
            .expect("session");
        database
            .mark_session_opened(&reading.name, 1_711_275_800)
            .expect("opened");

        let mut screen = OverviewScreen::new(Config::default());
        screen.reload(&database).expect("reload");
        (directory, database, screen)
    }

    fn render_buffer(screen: &mut OverviewScreen, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| screen.render(frame)).expect("draw");
        buffer_to_string(terminal.backend().buffer())
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ctrl_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }
}
