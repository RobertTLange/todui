use std::cmp::min;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::config::Config;
use crate::db::Database;
use crate::db::sessions::OverviewAggregateSnapshot;
use crate::domain::github::github_repo_url;
use crate::domain::pomodoro::{PomodoroKind, PomodoroRun, PomodoroState, remaining_seconds};
use crate::domain::session::{SessionOverview, normalize_session_name};
use crate::domain::todo::{Todo, TodoStatus};
use crate::error::Result;
use crate::timestamp::now_utc_timestamp;
use crate::timestamp::{format_compact_local, format_full_local, format_month_day_local};
use crate::tui::browser;
use crate::tui::input::{next_char_boundary, previous_char_boundary, resolved_text_char};
use crate::tui::layout::centered_rect;
use crate::tui::terminal::AppTerminal;
use crate::tui::theme::{SelectionTone, SurfaceTone, TextTone, Theme};
use crate::tui::widgets::details::{rect_contains, repo_hitbox, repo_line};
use crate::tui::widgets::editor::{EditorField, EditorView, editor_height, render_editor};
use crate::tui::widgets::markdown::{link_hitboxes, render_markdown};
use crate::tui::widgets::pomodoro::{active_footer, active_footer_height};

const EVENT_POLL_MS: u64 = 250;
const TAG_COLUMN_WIDTH: usize = 6;
const REV_COLUMN_WIDTH: usize = 5;
const OPEN_COLUMN_WIDTH: usize = 4;
const LAST_UPDATED_COLUMN_WIDTH: usize = 12;
const SESSION_COLUMN_SPACING: usize = 5;
const SESSION_COLUMN_SPACING_WITHOUT_REV: usize = 4;
const MIN_PREFERRED_SESSION_NAME_WIDTH: usize = 17;
const TODO_PREVIEW_TIME_WIDTH: usize = 11;
const OPEN_TODO_PREVIEW_MAX_ITEMS: usize = 10;
const SESSION_METADATA_WIDTH: u16 = 60;
const SESSION_METADATA_HEIGHT: u16 = 21;
const NOTES_EDITOR_WIDTH: u16 = 72;
const NOTES_EDITOR_HEIGHT: u16 = 18;
const NOTES_EDITOR_SEPARATOR_HEIGHT: u16 = 1;
const NOTES_EDITOR_FOOTER_HEIGHT: u16 = 2;
const NOTES_EDITOR_FOOTER_TEXT: &str =
    "Enter save  Shift+Enter newline  Ctrl+J newline\nArrows move  Esc cancel";
const WEEK_SECONDS: i64 = 7 * 24 * 60 * 60;
const OVERVIEW_LIST_PERCENT: u16 = 40;
const OVERVIEW_NOTES_PERCENT: u16 = 40;
const OVERVIEW_SUMMARY_PERCENT: u16 = 20;
const OVERVIEW_WIDE_LEFT_TOP_PERCENT: u16 = 50;
const OVERVIEW_WIDE_RIGHT_TOP_PERCENT: u16 = 33;
const OVERVIEW_WIDE_RIGHT_BOTTOM_PERCENT: u16 = 67;
const OVERVIEW_WIDE_PRIMARY_PERCENT: u16 = 58;
const OVERVIEW_WIDE_SECONDARY_PERCENT: u16 = 42;
const OVERVIEW_INLINE_POMODORO_MIN_WIDTH: u16 = 90;
pub fn run(database: &mut Database, config: &Config) -> Result<()> {
    super::run(database, config, super::TuiRoute::Overview)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OverviewNavigationState {
    selected_index: usize,
    scroll_offset: usize,
}

pub(crate) fn run_in_terminal(
    terminal: &mut AppTerminal,
    database: &mut Database,
    config: &Config,
    navigation: OverviewNavigationState,
) -> Result<OverviewExit> {
    let mut screen = OverviewScreen::new(config.clone(), navigation);
    screen.last_area = terminal.size()?.into();
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

        screen.expire_toast();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OverviewExit {
    Quit,
    OpenSession {
        session_name: String,
        navigation: OverviewNavigationState,
    },
}

#[derive(Debug)]
struct OverviewScreen {
    sessions: Vec<SessionOverview>,
    detail_sessions: Vec<ExpandedSessionState>,
    expanded_sessions: Vec<ExpandedSessionState>,
    current_summary: OverviewAggregateSnapshot,
    previous_week_summary: OverviewAggregateSnapshot,
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
    toast: Option<ToastState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OverviewOverlay {
    Help,
    SessionEditor(SessionEditorMode),
    GeneralNotesEditor,
    SessionMetadata,
    DeleteSession { name: String },
}

#[derive(Debug, Clone)]
struct ToastState {
    message: String,
    expires_at: Instant,
    tone: ToastTone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToastTone {
    Warning,
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
    name_cursor: usize,
    tag: String,
    tag_cursor: usize,
    repo: String,
    repo_cursor: usize,
    existing_sessions: Vec<String>,
    suggested_sessions: Vec<String>,
    selected_suggestion: usize,
    suggestion_selected: bool,
    footer_hint: String,
    focused_field: EditorField,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct GeneralNotesEditorState {
    text: String,
    cursor: usize,
    scroll_row: usize,
    scroll_col: usize,
}

impl Default for SessionEditorState {
    fn default() -> Self {
        Self {
            name: String::new(),
            name_cursor: 0,
            tag: String::new(),
            tag_cursor: 0,
            repo: String::new(),
            repo_cursor: 0,
            existing_sessions: Vec::new(),
            suggested_sessions: Vec::new(),
            selected_suggestion: 0,
            suggestion_selected: false,
            footer_hint: String::new(),
            focused_field: EditorField::Primary,
            error: None,
        }
    }
}

impl OverviewScreen {
    fn new(config: Config, navigation: OverviewNavigationState) -> Self {
        Self {
            sessions: Vec::new(),
            detail_sessions: Vec::new(),
            expanded_sessions: Vec::new(),
            current_summary: OverviewAggregateSnapshot::default(),
            previous_week_summary: OverviewAggregateSnapshot::default(),
            active_run: None,
            overview_notes: String::new(),
            has_any_sessions: false,
            selected_index: navigation.selected_index,
            scroll_offset: navigation.scroll_offset,
            theme: Theme::from_config(&config),
            config,
            last_area: Rect::default(),
            overlay: None,
            session_editor: SessionEditorState::default(),
            notes_editor: GeneralNotesEditorState::default(),
            toast: None,
        }
    }

    fn reload(&mut self, database: &Database) -> Result<()> {
        self.has_any_sessions = database.has_any_sessions()?;
        self.sessions = database.list_session_overview()?;
        self.current_summary = database.overview_aggregate_as_of(now_utc_timestamp())?;
        self.previous_week_summary =
            database.overview_aggregate_as_of(now_utc_timestamp() - WEEK_SECONDS)?;
        self.detail_sessions = self
            .sessions
            .iter()
            .map(|session| {
                Ok(ExpandedSessionState {
                    name: session.name.clone(),
                    todos: open_preview_todos(database.get_live_todos(session.id)?),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.active_run = database.get_active_pomodoro()?;
        self.overview_notes = database.get_overview_notes()?;
        self.selected_index = min(self.selected_index, self.sessions.len().saturating_sub(1));
        self.sync_expanded_sessions(database)?;
        if self.last_area.width == 0 || self.last_area.height == 0 {
            let max_scroll = self.display_rows().len().saturating_sub(1);
            self.scroll_offset = min(self.scroll_offset, max_scroll);
        } else {
            self.ensure_selection_visible(self.visible_rows());
        }
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
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => {
                        self.overlay = None;
                    }
                    KeyCode::Char('u') => self.open_selected_session_repo(),
                    _ => {}
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
                self.open_session_editor(database)?;
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
            KeyCode::Char('u') => {
                self.open_selected_session_repo();
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
            KeyCode::Right | KeyCode::Char('l') => self
                .selected_session_name()
                .map(|session_name| self.open_session_exit(session_name)),
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
            KeyCode::Up
                if self.session_name_autocomplete_is_active()
                    && !self.session_editor.suggested_sessions.is_empty() =>
            {
                self.move_session_editor_suggestion(-1);
                Ok(None)
            }
            KeyCode::Down
                if self.session_name_autocomplete_is_active()
                    && !self.session_editor.suggested_sessions.is_empty() =>
            {
                self.move_session_editor_suggestion(1);
                Ok(None)
            }
            KeyCode::Enter => self.submit_session_editor(database),
            KeyCode::Backspace => {
                self.backspace_session_editor_char();
                self.refresh_session_editor_suggestions();
                self.session_editor.error = None;
                Ok(None)
            }
            KeyCode::Left => {
                self.move_session_editor_cursor_left();
                self.session_editor.error = None;
                Ok(None)
            }
            KeyCode::Right => {
                self.move_session_editor_cursor_right();
                self.session_editor.error = None;
                Ok(None)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_session_editor_char(resolved_text_char(&key, character));
                self.refresh_session_editor_suggestions();
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
                self.insert_notes_editor_char('\n');
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
                self.insert_notes_editor_char('\n');
                Ok(None)
            }
            KeyCode::Backspace => {
                self.backspace_notes_editor_char();
                Ok(None)
            }
            KeyCode::Left => {
                self.move_notes_cursor_left();
                Ok(None)
            }
            KeyCode::Right => {
                self.move_notes_cursor_right();
                Ok(None)
            }
            KeyCode::Up => {
                self.move_notes_cursor_vertical(-1);
                Ok(None)
            }
            KeyCode::Down => {
                self.move_notes_cursor_vertical(1);
                Ok(None)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_notes_editor_char(resolved_text_char(&key, character));
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<OverviewExit> {
        match mouse.kind {
            MouseEventKind::ScrollUp if self.overlay.is_none() => self.move_selection(-1),
            MouseEventKind::ScrollDown if self.overlay.is_none() => self.move_selection(1),
            MouseEventKind::Down(MouseButton::Left) => {
                if matches!(self.overlay, Some(OverviewOverlay::SessionMetadata)) {
                    if self
                        .session_metadata_repo_hitbox()
                        .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
                    {
                        self.open_selected_session_repo();
                    }
                    return None;
                }
                if self.overlay.is_some() {
                    return None;
                }
                if self
                    .details_repo_hitbox()
                    .is_some_and(|area| rect_contains(area, mouse.column, mouse.row))
                {
                    self.open_selected_session_repo();
                    return None;
                }
                if let Some(hitbox) = self
                    .notes_link_hitboxes()
                    .into_iter()
                    .find(|hitbox| rect_contains(hitbox.area, mouse.column, mouse.row))
                {
                    self.open_note_url(&hitbox.url);
                    return None;
                }
                let list_area = self.list_area(self.last_area);
                if rect_contains(list_area, mouse.column, mouse.row)
                    && let Some(index) = list_row_index(list_area, self.scroll_offset, mouse.row)
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
        let root_areas = self.root_areas(frame.area());
        frame.render_widget(Block::default().style(self.theme.app_style()), frame.area());

        let clock = format_compact_local(now_utc_timestamp());
        frame.render_widget(self.top_bar(&clock), root_areas.top_bar);
        if let Some(run) = self.active_run.as_ref()
            && let Some(pomodoro_area) = root_areas.pomodoro
        {
            frame.render_widget(
                active_footer(&self.theme, run, now_utc_timestamp()),
                pomodoro_area,
            );
        }
        let body_areas = self.body_areas(root_areas.body);

        if self.sessions.is_empty() {
            frame.render_widget(self.empty_state(), body_areas.list);
            frame.render_widget(self.notes_panel(body_areas.notes), body_areas.notes);
            frame.render_widget(self.summary_panel(), body_areas.summary);
        } else {
            frame.render_widget(self.session_list(body_areas.list), body_areas.list);
            frame.render_widget(self.notes_panel(body_areas.notes), body_areas.notes);
            frame.render_widget(self.summary_panel(), body_areas.summary);
            if let Some(details_area) = body_areas.details {
                frame.render_widget(self.details_panel(details_area), details_area);
            }
        }

        if matches!(self.overlay, Some(OverviewOverlay::Help)) {
            let area = centered_rect(frame.area(), 58, 11);
            frame.render_widget(Clear, area);
            frame.render_widget(self.help_overlay(), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::SessionMetadata)) {
            let area = centered_rect(
                frame.area(),
                SESSION_METADATA_WIDTH,
                SESSION_METADATA_HEIGHT,
            );
            frame.render_widget(Clear, area);
            frame.render_widget(self.session_metadata_modal(), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::SessionEditor(_))) {
            let width = 60;
            let area = centered_rect(frame.area(), width, self.session_editor_modal_height(width));
            frame.render_widget(Clear, area);
            frame.render_widget(self.session_editor_modal(area.width), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::GeneralNotesEditor)) {
            let area = centered_rect(frame.area(), NOTES_EDITOR_WIDTH, NOTES_EDITOR_HEIGHT);
            self.sync_notes_editor_viewport(self.general_notes_editor_text_area(area));
            frame.render_widget(Clear, area);
            self.render_general_notes_editor(frame, area);
        }
        if let Some(OverviewOverlay::DeleteSession { name }) = &self.overlay {
            let area = centered_rect(frame.area(), 58, 9);
            frame.render_widget(Clear, area);
            frame.render_widget(self.delete_session_modal(name), area);
        }
        if let Some(toast) = &self.toast {
            let area = centered_rect(frame.area(), 50, 3);
            frame.render_widget(Clear, area);
            let surface_tone = match toast.tone {
                ToastTone::Warning => SurfaceTone::Notice,
            };
            let text_tone = match toast.tone {
                ToastTone::Warning => TextTone::Warning,
            };
            frame.render_widget(
                Paragraph::new(toast.message.clone())
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Notice")
                            .style(self.theme.surface_style(surface_tone))
                            .border_style(self.theme.surface_border_style(surface_tone))
                            .title_style(self.theme.surface_title_style(surface_tone)),
                    )
                    .style(
                        self.theme.surface_style(surface_tone).fg(self
                            .theme
                            .text_style(text_tone)
                            .fg
                            .unwrap_or(self.theme.fg_default)),
                    ),
                area,
            );
        }
    }

    fn top_bar(&self, clock: &str) -> Paragraph<'static> {
        let subtitle = if self.sessions.is_empty() {
            if self.has_any_sessions {
                String::from("No sessions with open todos")
            } else {
                String::from("No sessions yet")
            }
        } else {
            format!(
                "{} visible sessions | tag first, then last updated",
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
                .title(Line::from(format!("⏰ {clock}")).right_aligned())
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

    fn details_panel(&self, area: Rect) -> Paragraph<'static> {
        let text = self
            .selected_session_metadata_lines(
                OPEN_TODO_PREVIEW_MAX_ITEMS,
                usize::from(area.width.saturating_sub(2)),
            )
            .map(Text::from)
            .unwrap_or_else(|| Text::from("Select a session to inspect its summary."));

        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Session Details")
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
            self.rendered_notes(area).text
        };

        Paragraph::new(content)
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
        let previous = self.previous_week_summary;
        let tagged_share = percentage_of(stats.tagged_sessions, stats.total_sessions);
        let untagged_share = percentage_of(stats.untagged_sessions, stats.total_sessions);
        let open_share = percentage_of(stats.open_todos, stats.total_todos);
        let completed_share = percentage_of(stats.done_todos, stats.total_todos);
        Paragraph::new(vec![
            Line::from(format!(
                "sessions: {} ({})",
                stats.total_sessions,
                format_wow_delta(stats.total_sessions - previous.total_sessions)
            )),
            Line::from(format!(
                "tagged: {} ({}%, {})",
                stats.tagged_sessions,
                tagged_share,
                format_wow_delta(stats.tagged_sessions - previous.tagged_sessions)
            )),
            Line::from(format!(
                "untagged: {} ({}%)",
                stats.untagged_sessions, untagged_share
            )),
            Line::from(format!(
                "todos: {} ({})",
                stats.total_todos,
                format_wow_delta(stats.total_todos - previous.total_todos)
            )),
            Line::from(format!(
                "open: {} ({}%, {})",
                stats.open_todos,
                open_share,
                format_wow_delta(stats.open_todos - previous.open_todos())
            )),
            Line::from(format!(
                "completed: {} ({}%, {})",
                stats.done_todos,
                completed_share,
                format_wow_delta(stats.done_todos - previous.done_todos)
            )),
            Line::from(format!(
                "human open: {} ({})",
                stats.human_open_todos,
                format_wow_delta(stats.human_open_todos - previous.human_open_todos)
            )),
            Line::from(format!(
                "agent open: {} ({})",
                stats.agent_open_todos,
                format_wow_delta(stats.agent_open_todos - previous.agent_open_todos)
            )),
            Line::from(format!(
                "human completed: {} ({})",
                stats.human_completed_todos,
                format_wow_delta(stats.human_completed_todos - previous.human_completed_todos)
            )),
            Line::from(format!(
                "agent completed: {} ({})",
                stats.agent_completed_todos,
                format_wow_delta(stats.agent_completed_todos - previous.agent_completed_todos)
            )),
            Line::from(format!(
                "completion: {}% ({})",
                stats.completion_rate,
                format_percentage_point_wow(stats.completion_rate - previous.completion_rate())
            )),
            Line::from(format!(
                "avg rev: r{} ({})",
                stats.average_revision,
                format_wow_delta(
                    i64::from(stats.average_revision) - i64::from(previous.average_revision)
                )
            )),
        ])
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
            "Navigation: j/k, arrows, PageUp/PageDown\nExpand todos: Enter\nOpen session: Right, l\nOpen repo: u or click repo\nNew session: n\nEdit notes: m\nEdit session: e\nEdit session alias: t\nSession metadata: i\nDelete session: D\nPomodoro: p start/pause/resume focus\nBreaks: b short break, B long break\nCancel timer: c\nQuit: q or Esc\nClose help: h, q, or Esc",
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

    fn session_editor_modal_height(&self, width: u16) -> u16 {
        editor_height(&self.session_editor_view(), width)
    }

    fn session_editor_modal(&self, width: u16) -> Paragraph<'_> {
        render_editor(&self.theme, self.session_editor_view(), width)
    }

    fn session_editor_view(&self) -> EditorView<'_> {
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
                self.session_editor.footer_hint.as_str(),
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
        EditorView {
            title,
            primary_label,
            primary_value,
            primary_cursor: Some(self.session_editor.name_cursor),
            secondary_label,
            secondary_value,
            secondary_cursor: Some(self.session_editor.tag_cursor),
            tertiary_label,
            tertiary_value,
            tertiary_cursor: Some(self.session_editor.repo_cursor),
            tertiary_value_style: None,
            focused_field: self.session_editor.focused_field,
            error: self.session_editor.error.as_deref(),
            footer_hint,
        }
    }

    fn session_metadata_modal(&self) -> Paragraph<'static> {
        let modal_area = centered_rect(
            self.last_area,
            SESSION_METADATA_WIDTH,
            SESSION_METADATA_HEIGHT,
        );
        let mut lines = self
            .selected_session_metadata_lines(
                OPEN_TODO_PREVIEW_MAX_ITEMS,
                usize::from(modal_area.width.saturating_sub(2)),
            )
            .unwrap_or_else(|| {
                vec![
                    Line::from("Select a session to inspect its summary."),
                    Line::from(String::new()),
                    Line::from("Esc close"),
                ]
            });
        if self.selected_session_repo().is_some() {
            lines.push(Line::from(String::new()));
            lines.push(Line::from("u open repo  Esc close"));
        } else {
            lines.push(Line::from(String::new()));
            lines.push(Line::from("Esc close"));
        }

        Paragraph::new(Text::from(lines))
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

    fn render_general_notes_editor(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Edit General Notes")
            .style(self.theme.surface_style(SurfaceTone::Overlay))
            .border_style(self.theme.surface_border_style(SurfaceTone::Overlay))
            .title_style(self.theme.surface_title_style(SurfaceTone::Overlay));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let [text_area, separator_area, footer_area] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(NOTES_EDITOR_SEPARATOR_HEIGHT),
            Constraint::Length(NOTES_EDITOR_FOOTER_HEIGHT),
        ])
        .areas(inner);

        frame.render_widget(
            Paragraph::new(self.notes_editor.text.as_str())
                .scroll((
                    self.notes_editor.scroll_row.min(u16::MAX as usize) as u16,
                    self.notes_editor.scroll_col.min(u16::MAX as usize) as u16,
                ))
                .style(self.theme.surface_style(SurfaceTone::Overlay)),
            text_area,
        );
        frame.render_widget(
            Paragraph::new("-".repeat(separator_area.width as usize))
                .style(self.theme.surface_style(SurfaceTone::Overlay)),
            separator_area,
        );
        frame.render_widget(
            Paragraph::new(NOTES_EDITOR_FOOTER_TEXT)
                .wrap(Wrap { trim: false })
                .style(self.theme.surface_style(SurfaceTone::Overlay)),
            footer_area,
        );

        if let Some(cursor) = self.general_notes_cursor_position(text_area) {
            frame.set_cursor_position(cursor);
        }
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

    fn navigation_state(&self) -> OverviewNavigationState {
        OverviewNavigationState {
            selected_index: self.selected_index,
            scroll_offset: self.scroll_offset,
        }
    }

    fn open_session_exit(&self, session_name: String) -> OverviewExit {
        let mut navigation = self.navigation_state();
        if let Some(selected_index) = self
            .sessions
            .iter()
            .position(|session| session.name == session_name)
        {
            navigation.selected_index = selected_index;
        }
        OverviewExit::OpenSession {
            session_name,
            navigation,
        }
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
        self.body_areas(self.root_areas(area).body).list
    }

    fn rendered_notes(&self, area: Rect) -> crate::tui::widgets::markdown::RenderedTextBlock {
        render_markdown(
            &self.theme,
            &self.overview_notes,
            area.width.saturating_sub(2),
        )
    }

    fn body_areas(&self, body: Rect) -> OverviewBodyAreas {
        if body.width >= 90 {
            let columns = Layout::horizontal([
                Constraint::Percentage(OVERVIEW_WIDE_PRIMARY_PERCENT),
                Constraint::Percentage(OVERVIEW_WIDE_SECONDARY_PERCENT),
            ])
            .split(body);
            let left_column = Layout::vertical([
                Constraint::Percentage(OVERVIEW_WIDE_LEFT_TOP_PERCENT),
                Constraint::Percentage(100 - OVERVIEW_WIDE_LEFT_TOP_PERCENT),
            ])
            .split(columns[0]);
            let right_column = Layout::vertical([
                Constraint::Percentage(OVERVIEW_WIDE_RIGHT_TOP_PERCENT),
                Constraint::Percentage(OVERVIEW_WIDE_RIGHT_BOTTOM_PERCENT),
            ])
            .split(columns[1]);
            OverviewBodyAreas {
                list: left_column[0],
                notes: left_column[1],
                summary: right_column[1],
                details: Some(right_column[0]),
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

    fn root_areas(&self, area: Rect) -> OverviewRootAreas {
        if self.active_run.is_some() {
            if area.width >= OVERVIEW_INLINE_POMODORO_MIN_WIDTH {
                let outer = Layout::vertical([
                    Constraint::Length(active_footer_height()),
                    Constraint::Min(8),
                ])
                .split(area);
                let top = Layout::horizontal([
                    Constraint::Percentage(OVERVIEW_WIDE_PRIMARY_PERCENT),
                    Constraint::Percentage(OVERVIEW_WIDE_SECONDARY_PERCENT),
                ])
                .split(outer[0]);
                OverviewRootAreas {
                    top_bar: top[0],
                    pomodoro: Some(top[1]),
                    body: outer[1],
                }
            } else {
                let outer = Layout::vertical([
                    Constraint::Length(3),
                    Constraint::Length(active_footer_height()),
                    Constraint::Min(8),
                ])
                .split(area);
                OverviewRootAreas {
                    top_bar: outer[0],
                    pomodoro: Some(outer[1]),
                    body: outer[2],
                }
            }
        } else {
            let outer = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(area);
            OverviewRootAreas {
                top_bar: outer[0],
                pomodoro: None,
                body: outer[1],
            }
        }
    }

    fn summary_stats(&self) -> OverviewSummaryStats {
        let total_sessions = self.sessions.len() as i64;
        let tagged_sessions = self
            .sessions
            .iter()
            .filter(|session| session.tag.is_some())
            .count() as i64;
        let total_todos = self.current_summary.total_todos;
        let done_todos = self.current_summary.done_todos;
        let open_todos = total_todos - done_todos;
        let completion_rate = if total_todos == 0 {
            0
        } else {
            ((done_todos * 100) + (total_todos / 2)) / total_todos
        };
        let newest_updated_at = self
            .sessions
            .iter()
            .map(|session| session.updated_at)
            .max()
            .unwrap_or(0);
        let oldest_updated_at = self
            .sessions
            .iter()
            .map(|session| session.updated_at)
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
            human_open_todos: self.current_summary.human_open_todos,
            agent_open_todos: self.current_summary.agent_open_todos,
            human_completed_todos: self.current_summary.human_completed_todos,
            agent_completed_todos: self.current_summary.agent_completed_todos,
            completion_rate,
            newest_updated_at,
            oldest_updated_at,
            average_revision,
        }
    }

    fn open_session_editor(&mut self, database: &Database) -> Result<()> {
        self.overlay = Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create));
        self.session_editor = SessionEditorState {
            existing_sessions: database
                .list_sessions()?
                .into_iter()
                .map(|session| session.name)
                .collect(),
            footer_hint: session_create_footer_hint(&[], 0, false),
            ..SessionEditorState::default()
        };
        self.refresh_session_editor_suggestions();
        Ok(())
    }

    fn open_session_metadata_editor(&mut self) {
        let Some(session) = self.sessions.get(self.selected_index) else {
            return;
        };
        let name = session.name.clone();
        let tag = session.tag.clone().unwrap_or_default();
        let repo = session.repo.clone().unwrap_or_default();
        self.overlay = Some(OverviewOverlay::SessionEditor(
            SessionEditorMode::EditMetadata { name: name.clone() },
        ));
        self.session_editor = SessionEditorState {
            name_cursor: name.len(),
            tag_cursor: tag.len(),
            repo_cursor: repo.len(),
            name,
            tag,
            repo,
            footer_hint: String::from("Empty clears  Enter save  Esc cancel"),
            focused_field: EditorField::Primary,
            ..SessionEditorState::default()
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
            cursor: self.overview_notes.len(),
            scroll_row: 0,
            scroll_col: 0,
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

    fn insert_session_editor_char(&mut self, character: char) {
        match self.session_editor.focused_field {
            EditorField::Primary => insert_editor_char(
                &mut self.session_editor.name,
                &mut self.session_editor.name_cursor,
                character,
            ),
            EditorField::Secondary => insert_editor_char(
                &mut self.session_editor.tag,
                &mut self.session_editor.tag_cursor,
                character,
            ),
            EditorField::Tertiary => insert_editor_char(
                &mut self.session_editor.repo,
                &mut self.session_editor.repo_cursor,
                character,
            ),
        }
    }

    fn session_name_autocomplete_is_active(&self) -> bool {
        matches!(
            self.overlay,
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create))
        ) && matches!(self.session_editor.focused_field, EditorField::Primary)
    }

    fn refresh_session_editor_suggestions(&mut self) {
        if !matches!(
            self.overlay,
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create))
        ) {
            return;
        }

        let query = normalize_session_name(&self.session_editor.name);
        let mut suggestions = if query.is_empty() {
            Vec::new()
        } else {
            self.session_editor
                .existing_sessions
                .iter()
                .filter(|name| name.contains(&query))
                .cloned()
                .collect()
        };
        suggestions.sort_by_key(|name| if *name == query { 0 } else { 1 });
        suggestions.truncate(6);
        self.session_editor.suggested_sessions = suggestions;
        self.session_editor.selected_suggestion = 0;
        self.session_editor.suggestion_selected = false;
        self.session_editor.footer_hint = session_create_footer_hint(
            &self.session_editor.suggested_sessions,
            self.session_editor.selected_suggestion,
            self.session_editor.suggestion_selected,
        );
    }

    fn move_session_editor_suggestion(&mut self, delta: isize) {
        let suggestion_count = self.session_editor.suggested_sessions.len();
        if suggestion_count == 0 {
            return;
        }

        self.session_editor.selected_suggestion = if delta.is_negative() {
            self.session_editor
                .selected_suggestion
                .saturating_sub(delta.unsigned_abs())
        } else {
            min(
                self.session_editor.selected_suggestion + delta as usize,
                suggestion_count.saturating_sub(1),
            )
        };
        self.session_editor.suggestion_selected = true;
        self.session_editor.footer_hint = session_create_footer_hint(
            &self.session_editor.suggested_sessions,
            self.session_editor.selected_suggestion,
            self.session_editor.suggestion_selected,
        );
    }

    fn backspace_session_editor_char(&mut self) {
        match self.session_editor.focused_field {
            EditorField::Primary => backspace_editor_char(
                &mut self.session_editor.name,
                &mut self.session_editor.name_cursor,
            ),
            EditorField::Secondary => backspace_editor_char(
                &mut self.session_editor.tag,
                &mut self.session_editor.tag_cursor,
            ),
            EditorField::Tertiary => backspace_editor_char(
                &mut self.session_editor.repo,
                &mut self.session_editor.repo_cursor,
            ),
        }
    }

    fn move_session_editor_cursor_left(&mut self) {
        match self.session_editor.focused_field {
            EditorField::Primary => {
                self.session_editor.name_cursor = previous_char_boundary(
                    &self.session_editor.name,
                    self.session_editor.name_cursor,
                );
            }
            EditorField::Secondary => {
                self.session_editor.tag_cursor = previous_char_boundary(
                    &self.session_editor.tag,
                    self.session_editor.tag_cursor,
                );
            }
            EditorField::Tertiary => {
                self.session_editor.repo_cursor = previous_char_boundary(
                    &self.session_editor.repo,
                    self.session_editor.repo_cursor,
                );
            }
        }
    }

    fn move_session_editor_cursor_right(&mut self) {
        match self.session_editor.focused_field {
            EditorField::Primary => {
                self.session_editor.name_cursor =
                    next_char_boundary(&self.session_editor.name, self.session_editor.name_cursor);
            }
            EditorField::Secondary => {
                self.session_editor.tag_cursor =
                    next_char_boundary(&self.session_editor.tag, self.session_editor.tag_cursor);
            }
            EditorField::Tertiary => {
                self.session_editor.repo_cursor =
                    next_char_boundary(&self.session_editor.repo, self.session_editor.repo_cursor);
            }
        }
    }

    fn insert_notes_editor_char(&mut self, character: char) {
        self.notes_editor
            .text
            .insert(self.notes_editor.cursor, character);
        self.notes_editor.cursor += character.len_utf8();
    }

    fn backspace_notes_editor_char(&mut self) {
        if self.notes_editor.cursor == 0 {
            return;
        }

        let start = previous_char_boundary(&self.notes_editor.text, self.notes_editor.cursor);
        self.notes_editor
            .text
            .replace_range(start..self.notes_editor.cursor, "");
        self.notes_editor.cursor = start;
    }

    fn move_notes_cursor_left(&mut self) {
        self.notes_editor.cursor =
            previous_char_boundary(&self.notes_editor.text, self.notes_editor.cursor);
    }

    fn move_notes_cursor_right(&mut self) {
        self.notes_editor.cursor =
            next_char_boundary(&self.notes_editor.text, self.notes_editor.cursor);
    }

    fn move_notes_cursor_vertical(&mut self, delta: isize) {
        self.notes_editor.cursor =
            move_cursor_vertical(&self.notes_editor.text, self.notes_editor.cursor, delta);
    }

    fn general_notes_editor_text_area(&self, area: Rect) -> Rect {
        let inner = area.inner(ratatui::layout::Margin::new(1, 1));
        let [text_area, _separator_area, _footer_area] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(NOTES_EDITOR_SEPARATOR_HEIGHT),
            Constraint::Length(NOTES_EDITOR_FOOTER_HEIGHT),
        ])
        .areas(inner);
        text_area
    }

    fn sync_notes_editor_viewport(&mut self, text_area: Rect) {
        if text_area.width == 0 || text_area.height == 0 {
            self.notes_editor.scroll_row = 0;
            self.notes_editor.scroll_col = 0;
            return;
        }

        let (cursor_row, cursor_col) =
            note_cursor_row_col(&self.notes_editor.text, self.notes_editor.cursor);
        let visible_rows = usize::from(text_area.height);
        let visible_cols = usize::from(text_area.width);

        let min_row = cursor_row.saturating_add(1).saturating_sub(visible_rows);
        self.notes_editor.scroll_row = self.notes_editor.scroll_row.clamp(min_row, cursor_row);

        let min_col = cursor_col.saturating_add(1).saturating_sub(visible_cols);
        self.notes_editor.scroll_col = self.notes_editor.scroll_col.clamp(min_col, cursor_col);
    }

    fn general_notes_cursor_position(&self, text_area: Rect) -> Option<Position> {
        if text_area.width == 0 || text_area.height == 0 {
            return None;
        }

        let (cursor_row, cursor_col) =
            note_cursor_row_col(&self.notes_editor.text, self.notes_editor.cursor);
        let relative_row = cursor_row.checked_sub(self.notes_editor.scroll_row)?;
        let relative_col = cursor_col.checked_sub(self.notes_editor.scroll_col)?;

        if relative_row >= usize::from(text_area.height)
            || relative_col >= usize::from(text_area.width)
        {
            return None;
        }

        Some(Position::new(
            text_area.x + relative_col as u16,
            text_area.y + relative_row as u16,
        ))
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

                if let Some(existing_name) = self.selected_or_exact_existing_session(name) {
                    match database.mark_session_opened(&existing_name, now_utc_timestamp()) {
                        Ok(session) => {
                            self.reload(database)?;
                            self.close_session_editor();
                            return Ok(Some(self.open_session_exit(session.name)));
                        }
                        Err(error) => {
                            self.session_editor.error = Some(error.to_string());
                            return Ok(None);
                        }
                    }
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
                        Ok(Some(self.open_session_exit(session.name)))
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

    fn selected_or_exact_existing_session(&self, name: &str) -> Option<String> {
        let normalized = normalize_session_name(name);
        let exact = self
            .session_editor
            .existing_sessions
            .iter()
            .find(|session_name| **session_name == normalized)
            .cloned();
        if exact.is_some() {
            return exact;
        }

        if self.session_editor.suggestion_selected {
            self.session_editor
                .suggested_sessions
                .get(self.session_editor.selected_suggestion)
                .cloned()
        } else {
            None
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

    fn detail_todos_for_session(&self, session_name: &str) -> Option<&[Todo]> {
        self.detail_sessions
            .iter()
            .find(|session| session.name == session_name)
            .map(|session| session.todos.as_slice())
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

    fn selected_session_metadata_lines(
        &self,
        max_open_todos: usize,
        inner_width: usize,
    ) -> Option<Vec<Line<'static>>> {
        self.sessions.get(self.selected_index).map(|session| {
            let mut lines = vec![
                Line::from(format!("session: {}", session.name)),
                Line::from(format!(
                    "tag: {}",
                    session.tag.as_deref().unwrap_or("untagged")
                )),
                repo_line(&self.theme, session.repo.as_deref()),
                Line::from(format!(
                    "last updated: {}",
                    format_full_local(session.updated_at)
                )),
                Line::from(format!("current revision: r{}", session.current_revision)),
                Line::from(format!(
                    "open todos: {}",
                    session.todo_count - session.done_count
                )),
                Line::from(format!("done todos: {}", session.done_count)),
                Line::from(String::new()),
            ];
            lines.extend(session_open_todo_lines(
                &self.theme,
                self.selected_session_open_todos().unwrap_or(&[]),
                max_open_todos,
                inner_width,
            ));
            lines
        })
    }

    fn selected_session_open_todos(&self) -> Option<&[Todo]> {
        self.sessions
            .get(self.selected_index)
            .and_then(|session| self.detail_todos_for_session(&session.name))
    }

    fn selected_session_repo(&self) -> Option<&str> {
        self.sessions
            .get(self.selected_index)
            .and_then(|session| session.repo.as_deref())
    }

    fn details_repo_hitbox(&self) -> Option<Rect> {
        let body = self.body_areas(self.root_areas(self.last_area).body);
        repo_hitbox(body.details?, 2, self.selected_session_repo())
    }

    fn notes_link_hitboxes(&self) -> Vec<crate::tui::widgets::markdown::LinkHitbox> {
        if self.overview_notes.trim().is_empty() || self.last_area.height == 0 {
            return Vec::new();
        }

        let notes_area = self.body_areas(self.root_areas(self.last_area).body).notes;
        let rendered = self.rendered_notes(notes_area);
        link_hitboxes(notes_area, &rendered.links)
    }

    fn session_metadata_repo_hitbox(&self) -> Option<Rect> {
        repo_hitbox(
            centered_rect(
                self.last_area,
                SESSION_METADATA_WIDTH,
                SESSION_METADATA_HEIGHT,
            ),
            2,
            self.selected_session_repo(),
        )
    }

    fn open_selected_session_repo(&mut self) {
        let Some(repo) = self.selected_session_repo() else {
            return;
        };
        match github_repo_url(repo).and_then(|url| {
            browser::open_url(&url)?;
            Ok(url)
        }) {
            Ok(_) => {}
            Err(error) => {
                self.set_toast(format!("Failed to open repo: {error}"), ToastTone::Warning)
            }
        }
    }

    fn open_note_url(&mut self, url: &str) {
        if let Err(error) = browser::open_url(url) {
            self.set_toast(format!("Failed to open URL: {error}"), ToastTone::Warning);
        }
    }

    fn set_toast(&mut self, message: String, tone: ToastTone) {
        self.toast = Some(ToastState {
            message,
            expires_at: Instant::now() + Duration::from_secs(2),
            tone,
        });
    }

    fn expire_toast(&mut self) {
        if self
            .toast
            .as_ref()
            .is_some_and(|toast| Instant::now() >= toast.expires_at)
        {
            self.toast = None;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OverviewSummaryStats {
    total_sessions: i64,
    tagged_sessions: i64,
    untagged_sessions: i64,
    total_todos: i64,
    open_todos: i64,
    done_todos: i64,
    human_open_todos: i64,
    agent_open_todos: i64,
    human_completed_todos: i64,
    agent_completed_todos: i64,
    completion_rate: i64,
    newest_updated_at: i64,
    oldest_updated_at: i64,
    average_revision: u32,
}

#[derive(Debug, Clone, Copy)]
struct OverviewBodyAreas {
    list: Rect,
    notes: Rect,
    summary: Rect,
    details: Option<Rect>,
}

#[derive(Debug, Clone, Copy)]
struct OverviewRootAreas {
    top_bar: Rect,
    pomodoro: Option<Rect>,
    body: Rect,
}

impl OverviewAggregateSnapshot {
    fn open_todos(self) -> i64 {
        self.total_todos - self.done_todos
    }

    fn completion_rate(self) -> i64 {
        if self.total_todos == 0 {
            0
        } else {
            ((self.done_todos * 100) + (self.total_todos / 2)) / self.total_todos
        }
    }
}

fn format_delta(delta: i64) -> String {
    if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}

fn format_wow_delta(delta: i64) -> String {
    format!("{} WoW", format_delta(delta))
}

fn format_percentage_point_wow(delta: i64) -> String {
    format!("{} WoW", format_percentage_point_delta(delta))
}

fn format_percentage_point_delta(delta: i64) -> String {
    format!("{}pp", format_delta(delta))
}

fn percentage_of(part: i64, total: i64) -> i64 {
    if total == 0 {
        0
    } else {
        ((part * 100) + (total / 2)) / total
    }
}

fn session_header_line(theme: &Theme, inner_width: usize) -> Line<'static> {
    let widths = session_column_widths(inner_width);
    let mut spans = vec![
        Span::styled(
            fit_cell("Tag", widths.tag),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("Name", widths.name),
            theme.surface_title_style(SurfaceTone::Open),
        ),
    ];
    if widths.shows_revision() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            fit_cell("Rev", widths.rev),
            theme.surface_title_style(SurfaceTone::Open),
        ));
    }
    spans.extend([
        Span::raw(" "),
        Span::styled(
            fit_cell("□", widths.open),
            theme.surface_title_style(SurfaceTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell("Last Updated", widths.updated),
            theme.surface_title_style(SurfaceTone::Open),
        ),
    ]);
    Line::from(spans)
}

fn session_row_line(
    session: &SessionOverview,
    theme: &Theme,
    inner_width: usize,
    is_selected: bool,
) -> Line<'static> {
    let widths = session_column_widths(inner_width);
    let mut spans = vec![
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
    ];
    if widths.shows_revision() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            fit_cell(&format!("r{}", session.current_revision), widths.rev),
            theme.text_style(TextTone::Meta),
        ));
    }
    spans.extend([
        Span::raw(" "),
        Span::styled(
            checkbox_cell(
                &(session.todo_count - session.done_count).to_string(),
                widths.open,
            ),
            theme.text_style(TextTone::Open),
        ),
        Span::raw(" "),
        Span::styled(
            fit_cell(&format_month_day_local(session.updated_at), widths.updated),
            theme.text_style(TextTone::Muted),
        ),
    ]);
    let mut line = Line::from(spans);
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

fn session_open_todo_lines(
    theme: &Theme,
    todos: &[Todo],
    max_items: usize,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        "open todo list:",
        theme.text_style(TextTone::Meta),
    ))];

    if todos.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No open todos",
            theme.text_style(TextTone::Muted),
        )));
        return lines;
    }

    let visible_count = todos.len().min(max_items);
    lines.extend(todos.iter().take(visible_count).map(|todo| {
        Line::from(Span::styled(
            fit_cell_with_ellipsis(&format!("  [ ] {}", todo.title), inner_width),
            theme.text_style(TextTone::Focus),
        ))
    }));
    if todos.len() > visible_count {
        lines.push(Line::from(Span::styled(
            fit_cell_with_ellipsis(
                &format!("  ... and {} more", todos.len() - visible_count),
                inner_width,
            ),
            theme.text_style(TextTone::Muted),
        )));
    }

    lines
}

fn session_column_widths(inner_width: usize) -> SessionColumnWidths {
    let base_width = TAG_COLUMN_WIDTH + OPEN_COLUMN_WIDTH + LAST_UPDATED_COLUMN_WIDTH;
    let rev_visible = inner_width
        >= base_width
            + REV_COLUMN_WIDTH
            + SESSION_COLUMN_SPACING
            + MIN_PREFERRED_SESSION_NAME_WIDTH;
    let (rev_width, spacing_width) = if rev_visible {
        (REV_COLUMN_WIDTH, SESSION_COLUMN_SPACING)
    } else {
        (0, SESSION_COLUMN_SPACING_WITHOUT_REV)
    };
    let name_width = inner_width.saturating_sub(base_width + rev_width + spacing_width);
    SessionColumnWidths {
        tag: TAG_COLUMN_WIDTH,
        name: name_width.max(1),
        rev: rev_width,
        open: OPEN_COLUMN_WIDTH,
        updated: LAST_UPDATED_COLUMN_WIDTH,
    }
}

fn fit_cell(text: &str, width: usize) -> String {
    let mut value = text.chars().take(width).collect::<String>();
    let padding = width.saturating_sub(value.chars().count());
    value.push_str(&" ".repeat(padding));
    value
}

fn checkbox_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return fit_cell(text, width);
    }

    center_cell(text, width)
}

fn fit_cell_with_ellipsis(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let char_count = text.chars().count();
    if char_count <= width {
        return fit_cell(text, width);
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let mut value = text.chars().take(width - 3).collect::<String>();
    value.push_str("...");
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

fn center_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let value = text.chars().take(width).collect::<String>();
    let padding = width.saturating_sub(value.chars().count());
    let left_padding = padding / 2;
    let right_padding = padding.saturating_sub(left_padding);

    format!(
        "{}{}{}",
        " ".repeat(left_padding),
        value,
        " ".repeat(right_padding)
    )
}

fn session_create_footer_hint(
    suggestions: &[String],
    selected_index: usize,
    suggestion_selected: bool,
) -> String {
    let base_hint = if suggestion_selected {
        "Tab switch  Up/Down select  Enter resume selected  Esc cancel"
    } else {
        "Tab switch  Up/Down select  Enter create/resume exact  Esc cancel"
    };
    if suggestions.is_empty() {
        return base_hint.to_string();
    }

    let mut lines = vec![String::from("Existing sessions:")];
    lines.extend(suggestions.iter().enumerate().map(|(index, name)| {
        if suggestion_selected && index == selected_index {
            format!("> {name}")
        } else {
            format!("  {name}")
        }
    }));
    lines.push(String::new());
    lines.push(base_hint.to_string());
    lines.join("\n")
}

fn open_preview_todos(todos: Vec<Todo>) -> Vec<Todo> {
    todos
        .into_iter()
        .filter(|todo| matches!(todo.status, TodoStatus::Open))
        .collect()
}

fn move_cursor_vertical(text: &str, cursor: usize, delta: isize) -> usize {
    let line_starts = note_line_starts(text);
    let current_line = line_starts
        .partition_point(|start| *start <= cursor)
        .saturating_sub(1);
    let target_line = if delta.is_negative() {
        current_line.saturating_sub(delta.unsigned_abs())
    } else {
        min(
            current_line + delta as usize,
            line_starts.len().saturating_sub(1),
        )
    };

    if target_line == current_line {
        return cursor;
    }

    let current_column = text[line_starts[current_line]..cursor].chars().count();
    let target_start = line_starts[target_line];
    let target_end = if target_line + 1 < line_starts.len() {
        line_starts[target_line + 1].saturating_sub(1)
    } else {
        text.len()
    };

    target_start + byte_offset_for_char_column(&text[target_start..target_end], current_column)
}

fn note_cursor_row_col(text: &str, cursor: usize) -> (usize, usize) {
    let line_starts = note_line_starts(text);
    let row = line_starts
        .partition_point(|start| *start <= cursor)
        .saturating_sub(1);
    let col = text[line_starts[row]..cursor].chars().count();
    (row, col)
}

fn note_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, character) in text.char_indices() {
        if character == '\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn byte_offset_for_char_column(text: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }

    for (current_column, (index, _)) in text.char_indices().enumerate() {
        if current_column == column {
            return index;
        }
    }

    text.len()
}

fn insert_editor_char(text: &mut String, cursor: &mut usize, character: char) {
    text.insert(*cursor, character);
    *cursor += character.len_utf8();
}

fn backspace_editor_char(text: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }

    let start = previous_char_boundary(text, *cursor);
    text.replace_range(start..*cursor, "");
    *cursor = start;
}

#[derive(Debug, Clone, Copy)]
struct SessionColumnWidths {
    tag: usize,
    name: usize,
    rev: usize,
    open: usize,
    updated: usize,
}

impl SessionColumnWidths {
    fn shows_revision(self) -> bool {
        self.rev > 0
    }
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
    use ratatui::backend::{Backend, TestBackend};
    use ratatui::buffer::Buffer;
    use ratatui::layout::{Position, Rect};
    use ratatui::text::Line;

    use super::{
        NOTES_EDITOR_HEIGHT, NOTES_EDITOR_WIDTH, OverviewDisplayRow, OverviewExit,
        OverviewNavigationState, OverviewScreen, WEEK_SECONDS, list_row_index, note_cursor_row_col,
        session_header_line, session_row_line, todo_preview_line,
    };
    use crate::config::Config;
    use crate::db::Database;
    use crate::domain::pomodoro::PomodoroKind;
    use crate::domain::todo::Todo;
    use crate::domain::todo::TodoStatus;
    use crate::timestamp::{format_month_day_local, now_utc_timestamp};
    use crate::tui::browser::{
        reset_test_browser, set_test_browser_should_fail, take_test_browser_opened_urls,
    };
    use crate::tui::layout::centered_rect;
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
            Some(OverviewExit::OpenSession {
                session_name: String::from("writing-sprint"),
                navigation: OverviewNavigationState {
                    selected_index: 1,
                    scroll_offset: 0,
                },
            })
        );

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(exit.is_none());
        assert!(
            screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::Todo { .. }))
        );

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(exit.is_none());
        assert!(
            !screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::Todo { .. }))
        );
    }

    #[test]
    fn overview_enter_toggles_inline_todos_for_multiple_sessions() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("expand");
        assert!(
            screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::EmptyTodos(_)))
        );

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
        assert!(
            screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::Todo { .. }))
        );
        assert!(
            screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::EmptyTodos(_)))
        );

        screen
            .handle_key(&mut database, key(KeyCode::Up))
            .expect("move back");
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("collapse first");
        assert!(
            screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::Todo { .. }))
        );
        assert!(
            !screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::EmptyTodos(_)))
        );
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

        let buffer = render_buffer(&mut screen, 120, 40);
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
                created_by_kind: crate::domain::todo::TodoActorKind::Human,
                completed_by_kind: None,
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
        assert_eq!(
            exit,
            Some(OverviewExit::OpenSession {
                session_name: String::from("inbox"),
                navigation: OverviewNavigationState {
                    selected_index: 0,
                    scroll_offset: 0,
                },
            })
        );
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
    fn overview_new_session_exact_match_resumes_hidden_completed_session() {
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
        let completed_extra = database
            .create_session(
                "Finished Sprint Extra",
                Some("private"),
                None,
                1_711_275_800,
            )
            .expect("extra session");
        let completed_extra_todo = database
            .add_todo(
                &completed_extra.name,
                "Done extra task",
                "",
                None,
                1_711_275_810,
            )
            .expect("extra todo");
        database
            .set_todo_status(
                completed_extra_todo.id,
                Some(&completed_extra.name),
                TodoStatus::Done,
                1_711_275_820,
            )
            .expect("extra done");

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");
        assert!(screen.sessions.is_empty());

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        for character in "Finished Sprint".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("  finished-sprint"));

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(
            exit,
            Some(OverviewExit::OpenSession {
                session_name: String::from("finished-sprint"),
                navigation: OverviewNavigationState {
                    selected_index: 0,
                    scroll_offset: 0,
                },
            })
        );
        assert!(database.get_session_by_name("finished-sprint").is_ok());
        assert!(database.get_session_by_name("finished-sprint-1").is_err());
    }

    #[test]
    fn overview_new_session_partial_match_without_selection_creates_session() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let completed = database
            .create_session("Alpha Done", Some("private"), None, 1_711_275_700)
            .expect("session");
        let todo = database
            .add_todo(&completed.name, "Done task", "", None, 1_711_275_710)
            .expect("todo");
        database
            .set_todo_status(
                todo.id,
                Some(&completed.name),
                TodoStatus::Done,
                1_711_275_720,
            )
            .expect("done");

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        for character in "Al".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("alpha-done"));
        assert!(!buffer.contains("> alpha-done"));

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(
            exit,
            Some(OverviewExit::OpenSession {
                session_name: String::from("al"),
                navigation: OverviewNavigationState {
                    selected_index: 0,
                    scroll_offset: 0,
                },
            })
        );
        assert!(database.get_session_by_name("al").is_ok());
    }

    #[test]
    fn overview_new_session_suggestions_can_select_hidden_session() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        for (name, now) in [
            ("Alpha Done", 1_711_275_700),
            ("Alpine Done", 1_711_275_800),
        ] {
            let session = database
                .create_session(name, Some("private"), None, now)
                .expect("session");
            let todo = database
                .add_todo(&session.name, "Done task", "", None, now + 10)
                .expect("todo");
            database
                .set_todo_status(todo.id, Some(&session.name), TodoStatus::Done, now + 20)
                .expect("done");
        }

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");
        assert!(screen.sessions.is_empty());

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        for character in "Al".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("Existing sessions"));
        assert!(buffer.contains("alpha-done"));
        assert!(buffer.contains("alpine-done"));
        assert!(!buffer.contains("> alpha-done"));
        assert!(!buffer.contains("> alpine-done"));

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("select second suggestion");
        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("> alpha-done"));

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(
            exit,
            Some(OverviewExit::OpenSession {
                session_name: String::from("alpha-done"),
                navigation: OverviewNavigationState {
                    selected_index: 0,
                    scroll_offset: 0,
                },
            })
        );
    }

    #[test]
    fn overview_new_session_modal_fits_six_suggestions() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        for index in 0..6 {
            database
                .create_session(
                    &format!("Match {index}"),
                    Some("private"),
                    None,
                    1_711_275_700 + i64::from(index),
                )
                .expect("session");
        }

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");
        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        for character in "Match".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }

        let buffer = render_buffer(&mut screen, 120, 24);
        assert!(buffer.contains("match-0"));
        assert!(buffer.contains("match-5"));
        assert!(buffer.contains("Enter create/resume exact"));
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
        assert!(wide_buffer.contains("Last Updated"));
        assert!(wide_buffer.contains("last updated:"));
        assert!(wide_buffer.contains("private"));
        assert!(wide_buffer.contains("writing-sprint"));
        assert!(wide_buffer.contains("General Notes"));
        assert!(wide_buffer.contains("Press m to edit overview notes."));
        assert!(wide_buffer.contains("Summary"));
        assert!(wide_buffer.contains("sessions: 2 (0 WoW)"));
        assert!(wide_buffer.contains("tagged: 2 (100%, 0 WoW)"));
        assert!(wide_buffer.contains("untagged: 0 (0%)"));
        assert!(wide_buffer.contains("todos: 1 (0 WoW)"));
        assert!(wide_buffer.contains("open: 1 (100%, 0 WoW)"));
        assert!(wide_buffer.contains("completed: 0 (0%, 0 WoW)"));
        assert!(wide_buffer.contains("completion: 0% (0pp WoW)"));
        assert!(wide_buffer.contains("avg rev: r2 (0 WoW)"));
        assert!(!wide_buffer.contains("Pomodoro"));
        assert!(!wide_buffer.contains("Keys"));

        let narrow_buffer = render_buffer(&mut populated, 80, 20);
        assert!(narrow_buffer.contains("Sessions"));
        assert!(narrow_buffer.contains("General Notes"));
        assert!(narrow_buffer.contains("Summary"));
        assert!(!narrow_buffer.contains("Details"));

        let (_directory, database) = Database::open_temp().expect("database");
        let mut empty = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
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
    fn overview_session_list_hides_revision_column_before_truncating_name() {
        let (_directory, _database, screen) = seeded_overview_screen();

        let narrow = render_widget_buffer(50, 8, |frame| {
            frame.render_widget(screen.session_list(frame.area()), frame.area());
        });
        assert!(!narrow.contains("Rev"));
        assert!(!narrow.contains("r2"));
        assert!(narrow.contains("writing-sprint"));

        let wide = render_widget_buffer(51, 8, |frame| {
            frame.render_widget(screen.session_list(frame.area()), frame.area());
        });
        assert!(wide.contains("Rev"));
        assert!(wide.contains("r2"));
    }

    #[test]
    fn overview_uses_single_square_header_for_open_count_column() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let header = lines_to_string(&[session_header_line(&screen.theme, 42)]);
        let writing = screen
            .sessions
            .iter()
            .find(|session| session.name == "writing-sprint")
            .expect("writing sprint");
        let row = lines_to_string(&[session_row_line(writing, &screen.theme, 42, false)]);

        assert!(header.contains("□   "));
        assert!(!header.contains("■   "));
        assert!(row.contains(" 1  "));
    }

    #[test]
    fn overview_header_renders_clock_in_top_border() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let rendered = render_widget_buffer(40, 3, |frame| {
            frame.render_widget(screen.top_bar("12:34:56"), frame.area());
        });

        assert!(rendered.contains("12:34:56"));
        assert!(rendered.contains("Overview"));
    }

    #[test]
    fn overview_top_bar_mentions_last_updated_ordering() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let rendered = render_widget_buffer(80, 5, |frame| {
            frame.render_widget(screen.top_bar("12:34:56"), frame.area());
        });

        assert!(rendered.contains("tag first, then last updated"));
    }

    #[test]
    fn overview_metadata_lists_open_todos_between_summary_and_hints() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        let done_todo = database
            .add_todo("writing-sprint", "Done task", "", None, 1_711_275_910)
            .expect("todo");
        database
            .add_todo("writing-sprint", "Ship docs", "", None, 1_711_275_920)
            .expect("todo");
        database
            .set_todo_status(
                done_todo.id,
                Some("writing-sprint"),
                TodoStatus::Done,
                1_711_275_930,
            )
            .expect("mark done");
        screen.reload(&database).expect("reload");
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move to writing");

        let metadata = lines_to_string(
            &screen
                .selected_session_metadata_lines(10, 58)
                .expect("metadata lines"),
        );

        let done_todos_index = metadata.find("done todos: 1").expect("done todos");
        let open_todo_list_index = metadata.find("open todo list:").expect("list");
        let draft_spec_index = metadata.find("  [ ] Draft spec").expect("draft spec");
        let ship_docs_index = metadata.find("  [ ] Ship docs").expect("ship docs");
        assert!(done_todos_index < open_todo_list_index);
        assert!(open_todo_list_index < draft_spec_index);
        assert!(draft_spec_index < ship_docs_index);
        assert!(!metadata.contains("Done task"));
        assert!(!metadata.contains("Enter expands todos."));
    }

    #[test]
    fn overview_details_preview_shows_first_ten_open_todos_then_overflow_count() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();

        for index in 1..=10 {
            database
                .add_todo(
                    "writing-sprint",
                    &format!("Overflow {index:02}"),
                    "",
                    None,
                    1_711_275_920 + i64::from(index),
                )
                .expect("todo");
        }

        screen.reload(&database).expect("reload");
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move to writing");

        let metadata = lines_to_string(
            &screen
                .selected_session_metadata_lines(10, 58)
                .expect("metadata lines"),
        );

        assert!(metadata.contains("Draft spec"));
        assert!(metadata.contains("Overflow 09"));
        assert!(!metadata.contains("Overflow 10"));
        assert!(metadata.contains("... and 1 more"));
    }

    #[test]
    fn overview_details_preview_truncates_long_open_todo_to_single_row() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 92, 24);
        database
            .add_todo(
                "writing-sprint",
                "very-long-details-preview-title-abcdefghijklmnopqrstuvwxyz-suffix-hidden-1234567890",
                "",
                None,
                1_711_275_920,
            )
            .expect("todo");

        screen.reload(&database).expect("reload");
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move to writing");

        let metadata = lines_to_string(
            &screen
                .selected_session_metadata_lines(10, 48)
                .expect("metadata lines"),
        );

        assert!(metadata.contains("very-long-details-preview"));
        assert!(metadata.contains("..."));
        assert!(!metadata.contains("suffix-hidden-1234567890"));
    }

    #[test]
    fn overview_wide_layout_places_summary_below_details() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let body = screen.body_areas(Rect::new(0, 0, 120, 20));
        let details = body.details.expect("wide layout details");

        assert_eq!(body.list.x, body.notes.x);
        assert!(body.notes.y > body.list.y);
        assert_eq!(details.x, body.summary.x);
        assert_eq!(details.width, body.summary.width);
        assert!(body.summary.y > details.y);
        assert!(body.summary.height > details.height);
        assert!(body.summary.height > body.notes.height);
    }

    #[test]
    fn overview_narrow_layout_keeps_stacked_summary_without_details() {
        let (_directory, _database, screen) = seeded_overview_screen();
        let body = screen.body_areas(Rect::new(0, 0, 80, 20));

        assert!(body.details.is_none());
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

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
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

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
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

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
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
    fn overview_shows_active_pomodoro_in_top_region() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();

        database
            .start_pomodoro(PomodoroKind::ShortBreak, 300, 1_711_275_900)
            .expect("run");
        screen.reload(&database).expect("reload");

        let wide_rendered = render_buffer(&mut screen, 120, 24);
        assert!(wide_rendered.contains("Pomodoro"));
        assert!(wide_rendered.contains("SHORT BREAK"));
        assert!(!wide_rendered.contains("Linked:"));
        assert!(!wide_rendered.contains("No linked todo"));

        let wide_lines: Vec<_> = wide_rendered.lines().collect();
        let overview_line = line_index_containing(&wide_lines, "Overview").expect("overview line");
        let pomodoro_line = line_index_containing(&wide_lines, "Pomodoro").expect("pomodoro line");
        let sessions_line = line_index_containing(&wide_lines, "Sessions").expect("sessions line");
        assert_eq!(overview_line, pomodoro_line);
        assert!(pomodoro_line < sessions_line);

        let medium_rendered = render_buffer(&mut screen, 80, 20);
        assert!(medium_rendered.contains("Pomodoro"));
        let medium_lines: Vec<_> = medium_rendered.lines().collect();
        let medium_overview_line =
            line_index_containing(&medium_lines, "Overview").expect("overview line");
        let medium_pomodoro_line =
            line_index_containing(&medium_lines, "Pomodoro").expect("pomodoro line");
        let medium_sessions_line =
            line_index_containing(&medium_lines, "Sessions").expect("sessions line");
        assert!(medium_overview_line < medium_pomodoro_line);
        assert!(medium_pomodoro_line < medium_sessions_line);
    }

    #[test]
    fn overview_keeps_pomodoro_inline_until_eighty_nine_columns() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .start_pomodoro(PomodoroKind::ShortBreak, 300, 1_711_275_900)
            .expect("run");
        screen.reload(&database).expect("reload");

        let at_threshold = render_buffer(&mut screen, 90, 20);
        let at_threshold_lines: Vec<_> = at_threshold.lines().collect();
        let overview_line =
            line_index_containing(&at_threshold_lines, "Overview").expect("overview line");
        let pomodoro_line =
            line_index_containing(&at_threshold_lines, "Pomodoro").expect("pomodoro line");
        assert_eq!(overview_line, pomodoro_line);

        let below_threshold = render_buffer(&mut screen, 89, 20);
        let below_threshold_lines: Vec<_> = below_threshold.lines().collect();
        let below_overview_line =
            line_index_containing(&below_threshold_lines, "Overview").expect("overview line");
        let below_pomodoro_line =
            line_index_containing(&below_threshold_lines, "Pomodoro").expect("pomodoro line");
        assert!(below_overview_line < below_pomodoro_line);
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
        let mut screen = OverviewScreen::new(config, OverviewNavigationState::default());
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
        assert_eq!(stats.human_open_todos, 1);
        assert_eq!(stats.agent_open_todos, 0);
        assert_eq!(stats.human_completed_todos, 0);
        assert_eq!(stats.agent_completed_todos, 0);
        assert_eq!(stats.completion_rate, 0);
        assert_eq!(stats.newest_updated_at, 1_711_275_700);
        assert_eq!(stats.oldest_updated_at, 1_711_275_650);
        assert_eq!(stats.average_revision, 2);
    }

    #[test]
    fn overview_summary_panel_shows_week_over_week_deltas() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let now = now_utc_timestamp();
        let baseline_time = now - (8 * WEEK_SECONDS);
        let recent_time = now - (2 * 24 * 60 * 60);

        let baseline = database
            .create_session("Baseline Sprint", Some("work"), None, baseline_time)
            .expect("baseline session");
        database
            .add_todo_with_actor(
                &baseline.name,
                "Draft spec",
                "",
                None,
                crate::domain::todo::TodoActorKind::Human,
                baseline_time + 60,
            )
            .expect("baseline todo");

        let recent = database
            .create_session("Recent Sprint", Some("private"), None, recent_time)
            .expect("recent session");
        let recent_done = database
            .add_todo_with_actor(
                &recent.name,
                "Done task",
                "",
                None,
                crate::domain::todo::TodoActorKind::Agent,
                recent_time + 60,
            )
            .expect("recent done todo");
        database
            .add_todo_with_actor(
                &recent.name,
                "Open task",
                "",
                None,
                crate::domain::todo::TodoActorKind::Human,
                recent_time + 120,
            )
            .expect("recent open todo");
        database
            .set_todo_status_with_actor(
                recent_done.id,
                Some(&recent.name),
                TodoStatus::Done,
                crate::domain::todo::TodoActorKind::Agent,
                recent_time + 180,
            )
            .expect("mark done");

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");
        let rendered = render_buffer(&mut screen, 120, 24);

        assert!(rendered.contains("sessions: 2 (+1 WoW)"));
        assert!(rendered.contains("tagged: 2 (100%, +1 WoW)"));
        assert!(rendered.contains("untagged: 0 (0%)"));
        assert!(rendered.contains("todos: 3 (+2 WoW)"));
        assert!(rendered.contains("open: 2 (67%, +1 WoW)"));
        assert!(rendered.contains("completed: 1 (33%, +1 WoW)"));
        assert!(rendered.contains("human open: 2 (+1 WoW)"));
        assert!(rendered.contains("agent open: 0 (0 WoW)"));
        assert!(rendered.contains("human completed: 0 (0 WoW)"));
        assert!(rendered.contains("agent completed: 1 (+1 WoW)"));
        assert!(rendered.contains("completion: 33% (+33pp WoW)"));
        assert!(rendered.contains("avg rev: r3 (+1 WoW)"));
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
    fn overview_session_metadata_editor_moves_cursor_with_arrow_keys() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .update_session_repo("reading-sprint", Some("openai/codex"), 1_711_275_810)
            .expect("set repo");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('e')))
            .expect("open metadata editor");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in name");
        assert!(render_buffer(&mut screen, 120, 24).contains("Name: reading-sprin|t"));

        screen
            .handle_key(&mut database, key(KeyCode::Tab))
            .expect("focus tag");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in tag");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in tag again");
        assert!(render_buffer(&mut screen, 120, 24).contains("Tag: priva|te"));

        screen
            .handle_key(&mut database, key(KeyCode::Tab))
            .expect("focus repo");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in repo");
        assert!(render_buffer(&mut screen, 120, 24).contains("Repo: openai/code|x"));
    }

    #[test]
    fn overview_session_metadata_editor_edits_at_cursor_position() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .update_session_repo("reading-sprint", Some("openai/codex"), 1_711_275_810)
            .expect("set repo");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('e')))
            .expect("open metadata editor");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in name");
        screen
            .handle_key(&mut database, key(KeyCode::Char('x')))
            .expect("insert in name");

        screen
            .handle_key(&mut database, key(KeyCode::Tab))
            .expect("focus tag");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in tag");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in tag again");
        screen
            .handle_key(&mut database, key(KeyCode::Backspace))
            .expect("backspace in tag");

        screen
            .handle_key(&mut database, key(KeyCode::Tab))
            .expect("focus repo");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in repo");
        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left in repo again");
        screen
            .handle_key(&mut database, key(KeyCode::Char('y')))
            .expect("insert in repo");

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("save metadata");

        let session = database
            .get_session_by_name("reading-sprinxt")
            .expect("renamed session");
        assert_eq!(session.tag.as_deref(), Some("privte"));
        assert_eq!(session.repo.as_deref(), Some("openai/codyex"));
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
    fn overview_clicks_session_repo_in_inline_details_panel() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .update_session_repo(
                "reading-sprint",
                Some("https://github.com/ExampleOrg/todui-keymove"),
                1_711_275_900,
            )
            .expect("set repo");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);
        reset_test_browser();

        let hitbox = screen.details_repo_hitbox().expect("repo hitbox");
        let exit = screen.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            hitbox.x,
            hitbox.y,
        ));

        assert!(exit.is_none());
        assert_eq!(
            take_test_browser_opened_urls(),
            vec![String::from("https://github.com/exampleorg/todui-keymove")]
        );
    }

    #[test]
    fn overview_clicks_session_repo_in_metadata_popup() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .update_session_repo(
                "reading-sprint",
                Some("ExampleOrg/todui-keymove"),
                1_711_275_900,
            )
            .expect("set repo");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);
        reset_test_browser();

        screen
            .handle_key(&mut database, key(KeyCode::Char('i')))
            .expect("open popup");
        let hitbox = screen
            .session_metadata_repo_hitbox()
            .expect("popup repo hitbox");
        let exit = screen.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            hitbox.x,
            hitbox.y,
        ));

        assert!(exit.is_none());
        assert_eq!(
            take_test_browser_opened_urls(),
            vec![String::from("https://github.com/exampleorg/todui-keymove")]
        );
    }

    #[test]
    fn overview_clicks_detected_url_in_general_notes() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .save_overview_notes("Plan: https://example.com/docs.")
            .expect("save notes");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);
        reset_test_browser();

        let hitbox = screen
            .notes_link_hitboxes()
            .into_iter()
            .next()
            .expect("notes link hitbox")
            .area;
        let exit = screen.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            hitbox.x,
            hitbox.y,
        ));

        assert!(exit.is_none());
        assert_eq!(
            take_test_browser_opened_urls(),
            vec![String::from("https://example.com/docs")]
        );
    }

    #[test]
    fn overview_ignores_clicks_outside_general_notes_url_span() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .save_overview_notes("Plan: https://example.com/docs.")
            .expect("save notes");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);
        reset_test_browser();

        let hitbox = screen
            .notes_link_hitboxes()
            .into_iter()
            .next()
            .expect("notes link hitbox")
            .area;
        let exit = screen.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            hitbox.x.saturating_sub(1),
            hitbox.y,
        ));

        assert!(exit.is_none());
        assert!(take_test_browser_opened_urls().is_empty());
    }

    #[test]
    fn overview_keeps_newline_separated_note_urls_distinct() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .save_overview_notes("https://example.com/docs\nhttps://openai.com/research")
            .expect("save notes");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);
        reset_test_browser();

        let hitboxes = screen.notes_link_hitboxes();
        assert_eq!(hitboxes.len(), 2);

        let second = &hitboxes[1];
        let exit = screen.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            second.area.x,
            second.area.y,
        ));

        assert!(exit.is_none());
        assert_eq!(
            take_test_browser_opened_urls(),
            vec![String::from("https://openai.com/research")]
        );
    }

    #[test]
    fn overview_shows_notice_when_repo_open_fails() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        database
            .update_session_repo(
                "reading-sprint",
                Some("ExampleOrg/todui-keymove"),
                1_711_275_900,
            )
            .expect("set repo");
        screen.reload(&database).expect("reload");
        screen.last_area = Rect::new(0, 0, 120, 24);
        reset_test_browser();
        set_test_browser_should_fail(true);

        screen
            .handle_key(&mut database, key(KeyCode::Char('u')))
            .expect("open repo");

        assert!(
            screen
                .toast
                .as_ref()
                .expect("toast")
                .message
                .contains("Failed to open repo")
        );
        assert_eq!(
            take_test_browser_opened_urls(),
            vec![String::from("https://github.com/exampleorg/todui-keymove")]
        );
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
    fn overview_arrow_keys_move_within_notes_editor() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('m')))
            .expect("open notes editor");
        for character in "ab".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .expect("type first line");
        }
        screen
            .handle_key(&mut database, ctrl_key(KeyCode::Char('j')))
            .expect("newline");
        for character in "cd".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .expect("type second line");
        }

        screen
            .handle_key(&mut database, key(KeyCode::Left))
            .expect("move left");
        screen
            .handle_key(&mut database, key(KeyCode::Up))
            .expect("move up");
        screen
            .handle_key(&mut database, key(KeyCode::Char('X')))
            .expect("insert on first line");
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move down");
        screen
            .handle_key(&mut database, key(KeyCode::Right))
            .expect("move right");
        screen
            .handle_key(&mut database, key(KeyCode::Char('Y')))
            .expect("insert on second line");

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("save");

        assert_eq!(database.get_overview_notes().expect("notes"), "aXb\ncdY");
    }

    #[test]
    fn overview_general_notes_editor_scrolls_to_keep_last_lines_visible() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        let notes = (1..=20)
            .map(|index| format!("line {index:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        database.save_overview_notes(&notes).expect("save notes");
        screen.reload(&database).expect("reload");

        screen
            .handle_key(&mut database, key(KeyCode::Char('m')))
            .expect("open notes editor");

        let rendered = render_general_notes_editor_buffer(&mut screen, 120, 24);
        assert!(rendered.contains("line 20"));
        assert!(!rendered.contains("line 01"));
    }

    #[test]
    fn overview_general_notes_editor_scrolls_horizontally_with_cursor() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        let note = format!("prefix {}", "x".repeat(90));
        database.save_overview_notes(&note).expect("save notes");
        screen.reload(&database).expect("reload");

        screen
            .handle_key(&mut database, key(KeyCode::Char('m')))
            .expect("open notes editor");

        let rendered = render_general_notes_editor_buffer(&mut screen, 120, 24);
        assert!(rendered.contains(&"x".repeat(20)));
        assert!(!rendered.contains("prefix"));
    }

    #[test]
    fn overview_general_notes_editor_sets_terminal_cursor_position() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);
        database
            .save_overview_notes("top line\nmiddle line\nfinal spot")
            .expect("save notes");
        screen.reload(&database).expect("reload");

        screen
            .handle_key(&mut database, key(KeyCode::Char('m')))
            .expect("open notes editor");
        screen
            .handle_key(&mut database, key(KeyCode::Up))
            .expect("move to middle line");

        let cursor = render_cursor_position(&mut screen, 120, 24);
        let modal = centered_rect(
            Rect::new(0, 0, 120, 24),
            NOTES_EDITOR_WIDTH,
            NOTES_EDITOR_HEIGHT,
        );
        let text_area = screen.general_notes_editor_text_area(modal);
        let (cursor_row, cursor_col) =
            note_cursor_row_col(&screen.notes_editor.text, screen.notes_editor.cursor);
        assert_eq!(
            cursor,
            Position::new(
                text_area.x + (cursor_col - screen.notes_editor.scroll_col) as u16,
                text_area.y + (cursor_row - screen.notes_editor.scroll_row) as u16,
            )
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
    fn overview_restores_selection_after_opening_session() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .expect("move to writing");

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Right))
            .expect("open session");
        let Some(OverviewExit::OpenSession {
            session_name,
            navigation,
        }) = exit
        else {
            panic!("expected session open exit");
        };
        assert_eq!(session_name, "writing-sprint");

        let mut restored = OverviewScreen::new(Config::default(), navigation);
        restored.last_area = Rect::new(0, 0, 120, 24);
        restored
            .reload(&database)
            .expect("reload restored overview");

        assert_eq!(
            restored.selected_session_name().as_deref(),
            Some("writing-sprint")
        );
    }

    #[test]
    fn overview_return_does_not_reopen_expanded_session_rows() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .expect("expand");
        assert!(
            screen
                .display_rows()
                .iter()
                .any(|row| matches!(row, OverviewDisplayRow::EmptyTodos(_)))
        );

        let navigation = screen.navigation_state();
        let mut restored = OverviewScreen::new(Config::default(), navigation);
        restored.last_area = Rect::new(0, 0, 120, 24);
        restored
            .reload(&database)
            .expect("reload restored overview");

        assert!(restored.display_rows().iter().all(|row| !matches!(
            row,
            OverviewDisplayRow::Todo { .. } | OverviewDisplayRow::EmptyTodos(_)
        )));
    }

    #[test]
    fn overview_restores_scroll_offset_from_navigation_state() {
        let (_directory, database, mut screen) = seeded_overview_screen_with_count(12);
        screen.last_area = Rect::new(0, 0, 120, 8);
        for _ in 0..7 {
            screen.move_selection(1);
        }
        let selected_session_name = screen.selected_session_name();
        let scroll_offset = screen.scroll_offset;
        assert!(scroll_offset > 0);

        let mut restored = OverviewScreen::new(Config::default(), screen.navigation_state());
        restored.last_area = Rect::new(0, 0, 120, 8);
        restored
            .reload(&database)
            .expect("reload restored overview");

        assert_eq!(restored.selected_session_name(), selected_session_name);
        assert_eq!(restored.scroll_offset, scroll_offset);
    }

    #[test]
    fn overview_reload_with_unknown_viewport_keeps_saved_scroll_offset() {
        let (_directory, database, mut screen) = seeded_overview_screen_with_count(12);
        screen.last_area = Rect::new(0, 0, 120, 8);
        for _ in 0..7 {
            screen.move_selection(1);
        }
        let selected_index = screen.selected_index;
        let scroll_offset = screen.scroll_offset;
        let navigation = screen.navigation_state();

        let mut restored = OverviewScreen::new(Config::default(), navigation);
        restored
            .reload(&database)
            .expect("reload restored overview");

        assert_eq!(restored.selected_index, selected_index);
        assert_eq!(restored.scroll_offset, scroll_offset);
    }

    #[test]
    fn overview_navigation_state_clamps_when_selected_session_is_gone() {
        let (_directory, database, mut screen) = seeded_overview_screen_with_count(2);
        screen.last_area = Rect::new(0, 0, 120, 24);
        screen.move_selection(1);
        let mut navigation = screen.navigation_state();
        navigation.selected_index = 9;
        navigation.scroll_offset = 9;

        let mut restored = OverviewScreen::new(Config::default(), navigation);
        restored.last_area = Rect::new(0, 0, 120, 24);
        restored
            .reload(&database)
            .expect("reload restored overview");

        assert_eq!(restored.selected_index, 1);
        assert_eq!(restored.scroll_offset, 0);
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

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");
        (directory, database, screen)
    }

    fn seeded_overview_screen_with_count(
        count: usize,
    ) -> (tempfile::TempDir, Database, OverviewScreen) {
        let (directory, mut database) = Database::open_temp().expect("database");
        for index in 0..count {
            let session = database
                .create_session(
                    &format!("Session {index:02}"),
                    None,
                    None,
                    1_711_275_600 + index as i64,
                )
                .expect("session");
            database
                .add_todo(
                    &session.name,
                    &format!("Todo {index:02}"),
                    "",
                    None,
                    1_711_275_700 + index as i64,
                )
                .expect("todo");
        }

        let mut screen = OverviewScreen::new(Config::default(), OverviewNavigationState::default());
        screen.reload(&database).expect("reload");
        (directory, database, screen)
    }

    fn render_buffer(screen: &mut OverviewScreen, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| screen.render(frame)).expect("draw");
        buffer_to_string(terminal.backend().buffer())
    }

    fn render_widget_buffer(
        width: u16,
        height: u16,
        render: impl FnOnce(&mut ratatui::Frame<'_>),
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(render).expect("draw");
        buffer_to_string(terminal.backend().buffer())
    }

    fn render_cursor_position(screen: &mut OverviewScreen, width: u16, height: u16) -> Position {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| screen.render(frame)).expect("draw");
        terminal
            .backend_mut()
            .get_cursor_position()
            .expect("cursor")
    }

    fn render_general_notes_editor_buffer(
        screen: &mut OverviewScreen,
        width: u16,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| screen.render(frame)).expect("draw");
        let modal = centered_rect(
            Rect::new(0, 0, width, height),
            NOTES_EDITOR_WIDTH,
            NOTES_EDITOR_HEIGHT,
        );
        buffer_rect_to_string(terminal.backend().buffer(), modal)
    }

    fn buffer_to_string(buffer: &Buffer) -> String {
        buffer_rect_to_string(buffer, buffer.area)
    }

    fn buffer_rect_to_string(buffer: &Buffer, area: Rect) -> String {
        let mut lines = Vec::new();
        for y in area.y..area.y + area.height {
            let mut line = String::new();
            for x in area.x..area.x + area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn lines_to_string(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn line_index_containing(lines: &[&str], needle: &str) -> Option<usize> {
        lines.iter().position(|line| line.contains(needle))
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
