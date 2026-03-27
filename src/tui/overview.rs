use std::cmp::min;
use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::config::Config;
use crate::db::Database;
use crate::domain::session::SessionOverview;
use crate::error::Result;
use crate::timestamp::format_full_local;
use crate::timestamp::now_utc_timestamp;
use crate::tui::layout::centered_rect;
use crate::tui::terminal::AppTerminal;
use crate::tui::theme::Theme;
use crate::tui::widgets::editor::{EditorField, EditorView, render_editor};

const EVENT_POLL_MS: u64 = 250;

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
    SessionEditor,
    DeleteSession { slug: String, name: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SessionEditorState {
    name: String,
    error: Option<String>,
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
            Some(OverviewOverlay::SessionEditor) => {
                return self.handle_session_editor_key(database, key);
            }
            Some(OverviewOverlay::DeleteSession { .. }) => {
                return self.handle_delete_key(database, key);
            }
            None => {}
        }

        let exit = match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(OverviewExit::Quit),
            KeyCode::Char('n') => {
                self.open_session_editor();
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
            KeyCode::Enter => self.submit_session_editor(database),
            KeyCode::Backspace => {
                self.session_editor.name.pop();
                self.session_editor.error = None;
                Ok(None)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.session_editor.name.push(character);
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
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(frame.area());

        frame.render_widget(self.top_bar(), chunks[0]);

        if self.sessions.is_empty() {
            frame.render_widget(self.empty_state(), chunks[1]);
        } else {
            let body = if chunks[1].width >= 90 {
                Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                    .split(chunks[1])
            } else {
                Layout::horizontal([Constraint::Percentage(100)]).split(chunks[1])
            };

            let list_area = body[0];
            frame.render_stateful_widget(
                self.session_list(list_area.height),
                list_area,
                &mut self.list_state(),
            );
            if let Some(details_area) = body.get(1).copied() {
                frame.render_widget(self.details_panel(), details_area);
            }
        }

        frame.render_widget(self.footer(), chunks[2]);

        if matches!(self.overlay, Some(OverviewOverlay::SessionEditor)) {
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
            Line::from("todui | session overview"),
            Line::from(subtitle),
        ])
        .block(Block::default().borders(Borders::ALL).title("Overview"))
        .style(self.theme.block_style())
    }

    fn session_list(&self, height: u16) -> List<'static> {
        let visible_rows = self.visible_rows_for_height(height);
        let items = self
            .sessions
            .iter()
            .skip(self.scroll_offset)
            .take(visible_rows)
            .map(|session| ListItem::new(session_row(session)))
            .collect::<Vec<_>>();

        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Sessions"))
            .highlight_style(self.theme.selected_style().add_modifier(Modifier::BOLD))
    }

    fn details_panel(&self) -> Paragraph<'static> {
        let text = self
            .sessions
            .get(self.selected_index)
            .map(|session| {
                format!(
                    "name: {}\nslug: {}\nlast opened: {}\ncurrent revision: r{}\nopen todos: {}\ndone todos: {}\n\nEnter opens the session head.\nUse o inside the session to return here.\nUse H inside the session for revision history.",
                    session.name,
                    session.slug,
                    format_full_local(session.last_opened_at),
                    session.current_revision,
                    session.todo_count - session.done_count,
                    session.done_count
                )
            })
            .unwrap_or_else(|| String::from("Select a session to inspect its summary."));

        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .style(self.theme.block_style())
    }

    fn empty_state(&self) -> Paragraph<'static> {
        Paragraph::new("No sessions yet.\n\nPress n to create one from the TUI.")
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Sessions"))
            .style(self.theme.block_style())
    }

    fn footer(&self) -> Paragraph<'static> {
        Paragraph::new("j/k move  n new  D delete  Enter open  q quit")
            .block(Block::default().borders(Borders::ALL).title("Keys"))
            .style(self.theme.block_style())
    }

    fn session_editor_modal(&self) -> Paragraph<'_> {
        render_editor(
            &self.theme,
            EditorView {
                title: "New Session",
                primary_label: "Name",
                primary_value: &self.session_editor.name,
                secondary_label: None,
                secondary_value: None,
                focused_field: EditorField::Primary,
                error: self.session_editor.error.as_deref(),
                footer_hint: "Enter create  Esc cancel",
            },
        )
    }

    fn delete_session_modal(&self, slug: &str, name: &str) -> Paragraph<'static> {
        Paragraph::new(format!(
            "Delete session {name} ({slug})?\n\nThis permanently removes its todos, history, and pomodoro runs.\n\nEnter delete  Esc cancel"
        ))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Delete Session"))
        .style(self.theme.block_style())
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
        height.saturating_sub(2).max(1) as usize
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

    fn list_state(&self) -> ListState {
        let mut state = ListState::default();
        if !self.sessions.is_empty() {
            state.select(Some(self.selected_index.saturating_sub(self.scroll_offset)));
        }
        state
    }

    fn list_area(&self, area: Rect) -> Rect {
        let body = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area)[1];
        if body.width >= 90 {
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(body)
                [0]
        } else {
            body
        }
    }

    fn open_session_editor(&mut self) {
        self.overlay = Some(OverviewOverlay::SessionEditor);
        self.session_editor = SessionEditorState::default();
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
        let name = self.session_editor.name.trim();
        if name.is_empty() {
            self.session_editor.error = Some(String::from("Session name is required"));
            return Ok(None);
        }

        match database.create_session(name, None, now_utc_timestamp()) {
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
}

fn session_row(session: &SessionOverview) -> String {
    format!(
        "{} ({})  r{}  open:{} done:{}",
        session.name,
        session.slug,
        session.current_revision,
        session.todo_count - session.done_count,
        session.done_count
    )
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
    if y < inner_y || y >= list_area.bottom().saturating_sub(1) {
        return None;
    }

    Some(scroll_offset + usize::from(y.saturating_sub(inner_y)))
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

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(exit, Some(OverviewExit::OpenSession(String::from("inbox"))));
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
        let row = list_area.y + 2;

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
        let populated_buffer = render_buffer(&mut populated, 120, 24);
        assert!(populated_buffer.contains("session overview"));
        assert!(populated_buffer.contains("Writing Sprint"));
        assert!(populated_buffer.contains("Enter opens the session head."));
        assert!(populated_buffer.contains("return here."));

        let (_directory, database) = Database::open_temp().expect("database");
        let mut empty = OverviewScreen::new(Config::default());
        empty.reload(&database).expect("reload");
        let empty_buffer = render_buffer(&mut empty, 80, 20);
        assert!(empty_buffer.contains("No sessions yet."));
        assert!(empty_buffer.contains("Press n to create one"));
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
    fn list_row_index_uses_inner_rows_only() {
        let area = Rect::new(0, 0, 40, 10);
        assert_eq!(list_row_index(area, 0, 0), None);
        assert_eq!(list_row_index(area, 0, 1), Some(0));
        assert_eq!(list_row_index(area, 2, 2), Some(3));
    }

    fn seeded_overview_screen() -> (tempfile::TempDir, Database, OverviewScreen) {
        let (directory, mut database) = Database::open_temp().expect("database");
        let writing = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&writing.slug, "Draft spec", "", 1_711_275_650)
            .expect("todo");
        let reading = database
            .create_session("Reading Sprint", None, 1_711_275_700)
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
