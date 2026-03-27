use std::cmp::min;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};

use crate::config::Config;
use crate::db::Database;
use crate::domain::session::SessionOverview;
use crate::error::Result;
use crate::timestamp::now_utc_timestamp;
use crate::timestamp::{format_full_local, format_month_day_local};
use crate::tui::layout::centered_rect;
use crate::tui::terminal::AppTerminal;
use crate::tui::theme::{SelectionTone, SurfaceTone, TextTone, Theme};
use crate::tui::widgets::editor::{EditorField, EditorView, render_editor};

const EVENT_POLL_MS: u64 = 250;
const SUMMARY_PANEL_PERCENT: u16 = 15;
const SUMMARY_PANEL_MIN_HEIGHT: u16 = 8;

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
    selected_index: usize,
    scroll_offset: usize,
    theme: Theme,
    config: Config,
    last_area: Rect,
    overlay: Option<OverviewOverlay>,
    session_editor: SessionEditorState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OverviewOverlay {
    Help,
    SessionEditor(SessionEditorMode),
    DeleteSession { slug: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionEditorMode {
    Create,
    EditTag { slug: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionEditorState {
    name: String,
    tag: String,
    focused_field: EditorField,
    error: Option<String>,
}

impl Default for SessionEditorState {
    fn default() -> Self {
        Self {
            name: String::new(),
            tag: String::new(),
            focused_field: EditorField::Primary,
            error: None,
        }
    }
}

impl OverviewScreen {
    fn new(config: Config) -> Self {
        Self {
            sessions: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
            theme: Theme::from_config(&config),
            config,
            last_area: Rect::default(),
            overlay: None,
            session_editor: SessionEditorState::default(),
        }
    }

    fn reload(&mut self, database: &Database) -> Result<()> {
        self.sessions = database.list_session_overview()?;
        self.selected_index = min(self.selected_index, self.sessions.len().saturating_sub(1));
        self.ensure_selection_visible(self.visible_rows());
        Ok(())
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
            Some(OverviewOverlay::SessionEditor(_)) => {
                return self.handle_session_editor_key(database, key);
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
            KeyCode::Char('t') => {
                self.open_tag_editor();
                None
            }
            KeyCode::Char('D') => {
                self.open_delete_session();
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
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => {
                self.selected_session_slug().map(OverviewExit::OpenSession)
            }
            _ => None,
        };

        Ok(exit)
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
                if matches!(
                    self.overlay,
                    Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create))
                ) {
                    self.session_editor.focused_field = match self.session_editor.focused_field {
                        EditorField::Primary => EditorField::Secondary,
                        EditorField::Secondary => EditorField::Primary,
                    };
                }
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
                let slug = match &self.overlay {
                    Some(OverviewOverlay::DeleteSession { slug, .. }) => slug.clone(),
                    _ => return Ok(None),
                };
                database.delete_session(&slug)?;
                self.reload(database)?;
                self.overlay = None;
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
                    && index < self.sessions.len()
                {
                    self.selected_index = index;
                    self.ensure_selection_visible(self.visible_rows());
                }
            }
            _ => {}
        }
        None
    }

    fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        self.last_area = frame.area();
        let chunks =
            Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(frame.area());
        frame.render_widget(Block::default().style(self.theme.app_style()), frame.area());

        frame.render_widget(self.top_bar(), chunks[0]);

        if self.sessions.is_empty() {
            frame.render_widget(self.empty_state(), chunks[1]);
        } else {
            let (list_area, summary_area, details_area) = self.body_areas(frame.area());
            frame.render_stateful_widget(
                self.session_list(list_area.height),
                list_area,
                &mut self.list_state(),
            );
            frame.render_widget(self.summary_panel(), summary_area);
            if let Some(details_area) = details_area {
                frame.render_widget(self.details_panel(), details_area);
            }
        }

        if matches!(self.overlay, Some(OverviewOverlay::Help)) {
            let area = centered_rect(frame.area(), 58, 11);
            frame.render_widget(Clear, area);
            frame.render_widget(self.help_overlay(), area);
        }
        if matches!(self.overlay, Some(OverviewOverlay::SessionEditor(_))) {
            let area = centered_rect(frame.area(), 54, 8);
            frame.render_widget(Clear, area);
            frame.render_widget(self.session_editor_modal(), area);
        }
        if let Some(OverviewOverlay::DeleteSession { slug, name }) = &self.overlay {
            let area = centered_rect(frame.area(), 58, 9);
            frame.render_widget(Clear, area);
            frame.render_widget(self.delete_session_modal(slug, name), area);
        }
    }

    fn top_bar(&self) -> Paragraph<'static> {
        let subtitle = if self.sessions.is_empty() {
            String::from("No sessions yet")
        } else {
            format!(
                "{} sessions | newest first by last-opened timestamp",
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

    fn session_list(&self, height: u16) -> Table<'static> {
        let visible_rows = self.visible_rows_for_height(height);
        let rows = self
            .sessions
            .iter()
            .skip(self.scroll_offset)
            .take(visible_rows)
            .map(|session| session_table_row(session, &self.theme))
            .collect::<Vec<_>>();

        Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Fill(1),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(11),
            ],
        )
        .header(
            Row::new([
                Cell::from("Tag"),
                Cell::from("Slug"),
                Cell::from("Rev"),
                Cell::from("Open"),
                Cell::from("Done"),
                Cell::from("Last Opened"),
            ])
            .style(self.theme.surface_title_style(SurfaceTone::Open)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Sessions")
                .style(self.theme.surface_style(SurfaceTone::Neutral))
                .border_style(self.theme.surface_border_style(SurfaceTone::Open))
                .title_style(self.theme.surface_title_style(SurfaceTone::Open)),
        )
        .column_spacing(1)
        .row_highlight_style(self.theme.selection_style(SelectionTone::Open))
    }

    fn details_panel(&self) -> Paragraph<'static> {
        let text = self
            .sessions
            .get(self.selected_index)
            .map(|session| {
                format!(
                    "name: {}\nslug: {}\ntag: {}\nlast opened: {}\ncurrent revision: r{}\nopen todos: {}\ndone todos: {}\n\nEnter opens the session head.\nUse o inside the session to return here.\nUse H inside the session for revision history.",
                    session.name,
                    session.slug,
                    session.tag.as_deref().unwrap_or("untagged"),
                    format_full_local(session.last_opened_at),
                    session.current_revision,
                    session.todo_count - session.done_count,
                    session.done_count
                )
            })
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

    fn summary_panel(&self) -> Paragraph<'static> {
        let stats = self.summary_stats();
        let text = format!(
            "total sessions: {} | tagged: {} | untagged: {}\ntotal todos: {} | open: {} | completed: {}\ncompletion rate: {}%\nnewest opened: {}\noldest opened: {}\navg revision: r{}",
            stats.total_sessions,
            stats.tagged_sessions,
            stats.untagged_sessions,
            stats.total_todos,
            stats.open_todos,
            stats.done_todos,
            stats.completion_rate,
            format_month_day_local(stats.newest_last_opened_at),
            format_month_day_local(stats.oldest_last_opened_at),
            stats.average_revision
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
        Paragraph::new("No sessions yet.\n\nPress n to create one from the TUI.")
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
            "Navigation: j/k, arrows, PageUp/PageDown\nOpen session: Enter, Right, l\nNew session: n\nEdit tag: t\nDelete session: D\nQuit: q or Esc\nClose help: h, q, or Esc",
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
        let (title, primary_label, primary_value, secondary_label, secondary_value, footer_hint) =
            match &self.overlay {
                Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create)) => (
                    "New Session",
                    "Name",
                    self.session_editor.name.as_str(),
                    Some("Tag"),
                    Some(self.session_editor.tag.as_str()),
                    "Tab switch  Enter create  Esc cancel",
                ),
                Some(OverviewOverlay::SessionEditor(SessionEditorMode::EditTag { .. })) => (
                    "Edit Session Tag",
                    "Tag",
                    self.session_editor.tag.as_str(),
                    None,
                    None,
                    "Empty clears  Enter save  Esc cancel",
                ),
                _ => ("Session", "Value", "", None, None, "Esc cancel"),
            };
        render_editor(
            &self.theme,
            EditorView {
                title,
                primary_label,
                primary_value,
                secondary_label,
                secondary_value,
                focused_field: self.session_editor.focused_field,
                error: self.session_editor.error.as_deref(),
                footer_hint,
            },
        )
    }

    fn delete_session_modal(&self, slug: &str, name: &str) -> Paragraph<'static> {
        Paragraph::new(format!(
            "Delete session {name} ({slug})?\n\nThis permanently removes its todos, history, and pomodoro runs.\n\nEnter delete  Esc cancel"
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

    fn selected_session_slug(&self) -> Option<String> {
        self.sessions
            .get(self.selected_index)
            .map(|session| session.slug.clone())
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
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_index + 1 - visible_rows;
        }
    }

    fn list_state(&self) -> TableState {
        let mut state = TableState::default();
        if !self.sessions.is_empty() {
            state.select(Some(self.selected_index.saturating_sub(self.scroll_offset)));
        }
        state
    }

    fn list_area(&self, area: Rect) -> Rect {
        self.body_areas(area).0
    }

    fn body_areas(&self, area: Rect) -> (Rect, Rect, Option<Rect>) {
        let body = Layout::vertical([Constraint::Length(3), Constraint::Min(8)]).split(area)[1];
        if body.width >= 90 {
            let columns =
                Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                    .split(body);
            let left_column =
                Layout::vertical(self.summary_split(columns[0].height)).split(columns[0]);
            (left_column[0], left_column[1], Some(columns[1]))
        } else {
            let stacked = Layout::vertical(self.summary_split(body.height)).split(body);
            (stacked[0], stacked[1], None)
        }
    }

    fn summary_split(&self, total_height: u16) -> [Constraint; 2] {
        let proportional_height =
            ((u32::from(total_height) * u32::from(SUMMARY_PANEL_PERCENT)).div_ceil(100)) as u16;
        let summary_height = proportional_height.max(SUMMARY_PANEL_MIN_HEIGHT);
        let list_height = total_height.saturating_sub(summary_height).max(1);
        [
            Constraint::Length(list_height),
            Constraint::Length(summary_height),
        ]
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

    fn open_tag_editor(&mut self) {
        let Some(session) = self.sessions.get(self.selected_index) else {
            return;
        };
        self.overlay = Some(OverviewOverlay::SessionEditor(SessionEditorMode::EditTag {
            slug: session.slug.clone(),
            name: session.name.clone(),
        }));
        self.session_editor = SessionEditorState {
            name: String::new(),
            tag: session.tag.clone().unwrap_or_default(),
            focused_field: EditorField::Primary,
            error: None,
        };
    }

    fn open_delete_session(&mut self) {
        let Some(session) = self.sessions.get(self.selected_index) else {
            return;
        };
        self.overlay = Some(OverviewOverlay::DeleteSession {
            slug: session.slug.clone(),
            name: session.name.clone(),
        });
    }

    fn close_session_editor(&mut self) {
        self.overlay = None;
        self.session_editor = SessionEditorState::default();
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
                    None,
                    Some(self.session_editor.tag.as_str()),
                    now_utc_timestamp(),
                ) {
                    Ok(session) => {
                        self.reload(database)?;
                        self.close_session_editor();
                        Ok(Some(OverviewExit::OpenSession(session.slug)))
                    }
                    Err(error) => {
                        self.session_editor.error = Some(error.to_string());
                        Ok(None)
                    }
                }
            }
            Some(OverviewOverlay::SessionEditor(SessionEditorMode::EditTag { slug, .. })) => {
                match database.update_session_tag(
                    slug,
                    Some(self.session_editor.tag.as_str()),
                    now_utc_timestamp(),
                ) {
                    Ok(_) => {
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
            EditorField::Primary => {
                if matches!(
                    self.overlay,
                    Some(OverviewOverlay::SessionEditor(SessionEditorMode::Create))
                ) {
                    &mut self.session_editor.name
                } else {
                    &mut self.session_editor.tag
                }
            }
            EditorField::Secondary => &mut self.session_editor.tag,
        }
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

fn session_table_row(session: &SessionOverview, theme: &Theme) -> Row<'static> {
    Row::new([
        Cell::from(session.tag.clone().unwrap_or_else(|| String::from("-"))).style(
            if session.tag.is_some() {
                theme.text_style(TextTone::Tag)
            } else {
                theme.text_style(TextTone::Muted)
            },
        ),
        Cell::from(session.slug.clone()),
        Cell::from(format!("r{}", session.current_revision))
            .style(theme.text_style(TextTone::Meta)),
        Cell::from((session.todo_count - session.done_count).to_string())
            .style(theme.text_style(TextTone::Open)),
        Cell::from(session.done_count.to_string()).style(theme.text_style(TextTone::Completed)),
        Cell::from(format_month_day_local(session.last_opened_at))
            .style(theme.text_style(TextTone::Muted)),
    ])
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

    use super::{OverviewExit, OverviewScreen, list_row_index};
    use crate::config::Config;
    use crate::db::Database;

    #[test]
    fn overview_screen_handles_navigation_and_open() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        assert_eq!(
            screen.selected_session_slug().as_deref(),
            Some("reading-sprint")
        );

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        assert_eq!(
            screen.selected_session_slug().as_deref(),
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
        assert_eq!(
            exit,
            Some(OverviewExit::OpenSession(String::from("writing-sprint")))
        );
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
                .get_session_by_slug("inbox")
                .expect("new session")
                .tag
                .as_deref(),
            Some("private")
        );
        assert_eq!(
            database
                .get_session_by_slug("inbox")
                .expect("new session")
                .name,
            "Inbox"
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
        assert!(database.get_session_by_slug("inbox").is_err());
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
            screen.selected_session_slug().as_deref(),
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
        assert!(wide_buffer.contains("Slug"));
        assert!(wide_buffer.contains("Last Opened"));
        assert!(wide_buffer.contains("private"));
        assert!(wide_buffer.contains("writing-sprint"));
        assert!(wide_buffer.contains("Summary"));
        assert!(wide_buffer.contains("total sessions: 2"));
        assert!(wide_buffer.contains("tagged: 2"));
        assert!(wide_buffer.contains("untagged: 0"));
        assert!(wide_buffer.contains("total todos: 1"));
        assert!(wide_buffer.contains("open: 1"));
        assert!(wide_buffer.contains("completed: 0"));
        assert!(wide_buffer.contains("completion rate: 0%"));
        assert!(wide_buffer.contains("newest opened:"));
        assert!(wide_buffer.contains("oldest opened:"));
        assert!(wide_buffer.contains("avg revision: r2"));
        assert!(wide_buffer.contains("Enter opens the session head."));
        assert!(wide_buffer.contains("return here."));
        assert!(!wide_buffer.contains("Keys"));

        let narrow_buffer = render_buffer(&mut populated, 80, 20);
        assert!(narrow_buffer.contains("Sessions"));
        assert!(narrow_buffer.contains("Summary"));
        assert!(!narrow_buffer.contains("Details"));

        let (_directory, database) = Database::open_temp().expect("database");
        let mut empty = OverviewScreen::new(Config::default());
        empty.reload(&database).expect("reload");
        let empty_buffer = render_buffer(&mut empty, 80, 20);
        assert!(empty_buffer.contains("No sessions yet."));
        assert!(empty_buffer.contains("Press n to create one"));
        assert!(empty_buffer.contains("h = help"));
        assert!(!empty_buffer.contains("Summary"));
        assert!(!empty_buffer.contains("Keys"));
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
        assert!(render_buffer(&mut screen, 120, 24).contains("Open session"));

        screen.handle_key(&mut database, key(KeyCode::Esc)).unwrap();
        assert!(!render_buffer(&mut screen, 120, 24).contains("Open session"));
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
        assert!(database.get_session_by_slug("reading-sprint").is_err());
        assert_eq!(
            screen.selected_session_slug().as_deref(),
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
            screen.selected_session_slug().as_deref(),
            Some("reading-sprint")
        );
        assert!(database.get_session_by_slug("reading-sprint").is_ok());
    }

    #[test]
    fn overview_edits_selected_session_tag() {
        let (_directory, mut database, mut screen) = seeded_overview_screen();
        screen.last_area = Rect::new(0, 0, 120, 24);

        screen
            .handle_key(&mut database, key(KeyCode::Char('t')))
            .unwrap();
        assert!(render_buffer(&mut screen, 120, 24).contains("Edit Session Tag"));
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
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();

        assert_eq!(
            database
                .get_session_by_slug("reading-sprint")
                .expect("session")
                .tag
                .as_deref(),
            Some("deep-work")
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
            .create_session("Writing Sprint", None, Some("work"), 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.slug, "Draft spec", "", 1_711_275_650)
            .expect("todo");
        let reading = database
            .create_session("Reading Sprint", None, Some("private"), 1_711_275_700)
            .expect("session");
        database
            .mark_session_opened(&reading.slug, 1_711_275_800)
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

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }
}
