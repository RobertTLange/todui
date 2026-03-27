use std::cmp::min;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Borders, Cell, Clear, Gauge, List, ListItem, ListState, Paragraph, Row, Table,
    TableState, Wrap,
};

use crate::config::Config;
use crate::db::Database;
use crate::domain::pomodoro::{
    PomodoroKind, PomodoroRun, PomodoroState, PomodoroSummary, progress_ratio, remaining_seconds,
};
use crate::domain::revision::{RevisionMode, RevisionSummary, RevisionTodo, SessionSnapshot};
use crate::domain::session::SessionHeadToken;
use crate::domain::todo::TodoStatus;
use crate::error::Result;
use crate::timestamp::{format_full_local, format_month_day_local, now_utc_timestamp};
use crate::tui::layout::{LayoutMode, centered_rect, split_screen};
use crate::tui::terminal::AppTerminal;
use crate::tui::theme::Theme;
use crate::tui::widgets::editor::{EditorField, EditorView, render_editor};

const EVENT_POLL_MS: u64 = 250;

pub fn run(
    database: &mut Database,
    config: &Config,
    session_slug: Option<String>,
    revision: Option<u32>,
) -> Result<()> {
    super::run(
        database,
        config,
        super::TuiRoute::Session {
            session_slug,
            revision,
        },
    )
}

pub(crate) fn run_in_terminal(
    terminal: &mut AppTerminal,
    database: &mut Database,
    config: &Config,
    session_slug: Option<String>,
    revision: Option<u32>,
) -> Result<SessionExit> {
    let resolved_slug = database.resolve_session_slug(session_slug.as_deref())?;
    database.mark_session_opened(&resolved_slug, now_utc_timestamp())?;

    let mut screen = SessionScreen::new(resolved_slug, revision, config.clone());
    screen.reload(database)?;
    event_loop(terminal, database, &mut screen)
}

fn event_loop(
    terminal: &mut AppTerminal,
    database: &mut Database,
    screen: &mut SessionScreen,
) -> Result<SessionExit> {
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
                    screen.handle_mouse(database, terminal.size()?.into(), mouse_event)?
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionExit {
    Quit,
    Overview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Overlay {
    Help,
    History,
    Details,
    TodoEditor,
    DeleteTodo { id: i64, title: String },
    DeleteSession { slug: String, name: String },
}

#[derive(Debug, Clone)]
struct ToastState {
    message: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TodoEditorState {
    mode: TodoEditorMode,
    title: String,
    notes: String,
    focused_field: EditorField,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TodoEditorMode {
    #[default]
    Create,
    Edit {
        todo_id: i64,
    },
}

#[derive(Debug)]
struct SessionScreen {
    session_slug: String,
    revision: Option<u32>,
    snapshot: Option<SessionSnapshot>,
    revisions: Vec<RevisionSummary>,
    pomodoro_summary: PomodoroSummary,
    active_run: Option<PomodoroRun>,
    head_token: Option<SessionHeadToken>,
    selected_index: usize,
    scroll_offset: usize,
    history_index: usize,
    medium_drawer_open: bool,
    overlay: Option<Overlay>,
    todo_editor: TodoEditorState,
    toast: Option<ToastState>,
    theme: Theme,
    config: Config,
}

impl SessionScreen {
    fn new(session_slug: String, revision: Option<u32>, config: Config) -> Self {
        Self {
            session_slug,
            revision,
            snapshot: None,
            revisions: Vec::new(),
            pomodoro_summary: PomodoroSummary {
                completed_focus_runs: 0,
                total_focus_seconds: 0,
            },
            active_run: None,
            head_token: None,
            selected_index: 0,
            scroll_offset: 0,
            history_index: 0,
            medium_drawer_open: true,
            overlay: None,
            todo_editor: TodoEditorState::default(),
            toast: None,
            theme: Theme::from_config(&config),
            config,
        }
    }

    fn reload(&mut self, database: &Database) -> Result<()> {
        let selected_todo_id = self.current_todo().map(|todo| todo.todo_id);
        let snapshot = database.load_snapshot(&self.session_slug, self.revision)?;
        self.revisions = database.list_revisions(&self.session_slug)?;
        self.pomodoro_summary = database.pomodoro_summary_for_session(
            snapshot.session.id,
            self.revision.map(|_| snapshot.revision.created_at),
        )?;
        self.active_run = database.get_active_pomodoro()?;
        self.head_token = Some(database.session_head_token(&self.session_slug)?);
        self.snapshot = Some(snapshot);
        self.reselect(selected_todo_id);
        if let Some(revision) = self.revision {
            self.history_index = self
                .revisions
                .iter()
                .position(|candidate| candidate.revision_number == revision)
                .unwrap_or(0);
        }
        Ok(())
    }

    fn handle_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<SessionExit>> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Ok(Some(SessionExit::Quit));
        }

        match self.overlay {
            Some(Overlay::History) => return self.handle_history_key(database, key),
            Some(Overlay::Help) => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')
                ) {
                    self.overlay = None;
                }
                return Ok(None);
            }
            Some(Overlay::TodoEditor) => return self.handle_todo_editor_key(database, key),
            Some(Overlay::DeleteTodo { .. } | Overlay::DeleteSession { .. }) => {
                return self.handle_delete_key(database, key);
            }
            Some(Overlay::Details) => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                    self.overlay = None;
                }
                return Ok(None);
            }
            None => {}
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(Some(SessionExit::Quit)),
            KeyCode::Left | KeyCode::Char('o') => Ok(Some(SessionExit::Overview)),
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                Ok(None)
            }
            KeyCode::Up | KeyCode::Char('k')
                if matches!(key.code, KeyCode::Up)
                    || key_matches_binding(&key, &self.config.keys.up) =>
            {
                self.move_selection(-1, self.visible_rows());
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j')
                if matches!(key.code, KeyCode::Down)
                    || key_matches_binding(&key, &self.config.keys.down) =>
            {
                self.move_selection(1, self.visible_rows());
                Ok(None)
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_index = 0;
                self.scroll_offset = 0;
                Ok(None)
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_index = self.snapshot().todos.len().saturating_sub(1);
                self.ensure_selection_visible(self.visible_rows());
                Ok(None)
            }
            KeyCode::PageUp => {
                self.move_selection(-(self.visible_rows() as isize), self.visible_rows());
                Ok(None)
            }
            KeyCode::PageDown => {
                self.move_selection(self.visible_rows() as isize, self.visible_rows());
                Ok(None)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-(self.visible_rows() as isize), self.visible_rows());
                Ok(None)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(self.visible_rows() as isize, self.visible_rows());
                Ok(None)
            }
            KeyCode::Char('x') | KeyCode::Char(' ')
                if key_matches_binding(&key, &self.config.keys.toggle_done) =>
            {
                self.toggle_selected_todo(database)?;
                Ok(None)
            }
            KeyCode::Char('n') => {
                self.open_todo_editor();
                Ok(None)
            }
            KeyCode::Char('e') => {
                self.open_selected_todo_editor();
                Ok(None)
            }
            KeyCode::Char('d') => {
                self.open_delete_todo();
                Ok(None)
            }
            KeyCode::Char('D') => {
                self.open_delete_session();
                Ok(None)
            }
            KeyCode::Char('H') if key_matches_binding(&key, &self.config.keys.history) => {
                self.overlay = Some(Overlay::History);
                Ok(None)
            }
            KeyCode::Char('r') if self.revision.is_some() => {
                self.revision = None;
                self.reload(database)?;
                Ok(None)
            }
            KeyCode::Char('p') if key_matches_binding(&key, &self.config.keys.pomodoro) => {
                self.handle_pomodoro(database, PomodoroKind::Focus)?;
                Ok(None)
            }
            KeyCode::Char('b') => {
                self.handle_pomodoro(database, PomodoroKind::ShortBreak)?;
                Ok(None)
            }
            KeyCode::Char('B') => {
                self.handle_pomodoro(database, PomodoroKind::LongBreak)?;
                Ok(None)
            }
            KeyCode::Char('c') => {
                self.cancel_active_pomodoro(database)?;
                Ok(None)
            }
            KeyCode::Tab => {
                self.medium_drawer_open = !self.medium_drawer_open;
                Ok(None)
            }
            KeyCode::Enter => {
                if matches!(
                    split_screen(
                        Rect::new(0, 0, 60, 24),
                        self.medium_drawer_open,
                        self.top_bar_height(),
                        4,
                    )
                    .mode,
                    LayoutMode::Narrow
                ) {
                    self.overlay = Some(Overlay::Details);
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_todo_editor_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<SessionExit>> {
        match key.code {
            KeyCode::Esc => {
                self.close_todo_editor();
                Ok(None)
            }
            KeyCode::Tab => {
                self.todo_editor.focused_field = match self.todo_editor.focused_field {
                    EditorField::Primary => EditorField::Secondary,
                    EditorField::Secondary => EditorField::Primary,
                };
                Ok(None)
            }
            KeyCode::Enter => {
                self.submit_todo_editor(database)?;
                Ok(None)
            }
            KeyCode::Backspace => {
                let field = self.focused_todo_field();
                field.pop();
                self.todo_editor.error = None;
                Ok(None)
            }
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let field = self.focused_todo_field();
                field.push(character);
                self.todo_editor.error = None;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_history_key(
        &mut self,
        database: &Database,
        key: KeyEvent,
    ) -> Result<Option<SessionExit>> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
            KeyCode::Up | KeyCode::Char('k') => {
                self.history_index = self.history_index.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.history_index = min(
                    self.history_index + 1,
                    self.revisions.len().saturating_sub(1),
                );
            }
            KeyCode::Enter => {
                if let Some(revision) = self.revisions.get(self.history_index) {
                    self.revision = Some(revision.revision_number);
                    self.overlay = None;
                    self.reload(database)?;
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn handle_delete_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<SessionExit>> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
                Ok(None)
            }
            KeyCode::Enter => {
                let overlay = self.overlay.clone();
                match overlay {
                    Some(Overlay::DeleteTodo { id, .. }) => {
                        database.delete_todo(id, Some(&self.session_slug), now_utc_timestamp())?;
                        self.reload(database)?;
                        self.overlay = None;
                        self.set_toast(String::from("Todo deleted"));
                        Ok(None)
                    }
                    Some(Overlay::DeleteSession { slug, .. }) => {
                        database.delete_session(&slug)?;
                        self.overlay = None;
                        Ok(Some(SessionExit::Overview))
                    }
                    _ => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    fn handle_mouse(
        &mut self,
        database: &mut Database,
        area: Rect,
        mouse: MouseEvent,
    ) -> Result<()> {
        if matches!(
            self.overlay,
            Some(Overlay::TodoEditor | Overlay::Help | Overlay::Details)
        ) || matches!(
            self.overlay,
            Some(Overlay::DeleteTodo { .. } | Overlay::DeleteSession { .. })
        ) {
            return Ok(());
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.move_selection(-1, self.visible_rows()),
            MouseEventKind::ScrollDown => self.move_selection(1, self.visible_rows()),
            MouseEventKind::Down(MouseButton::Left) => {
                if matches!(self.overlay, Some(Overlay::History)) {
                    self.handle_history_click(database, area, mouse.row)?;
                    return Ok(());
                }
                let layout = self.layout_for_area(area);
                if let Some(target) =
                    list_click_target(layout.list, self.scroll_offset, mouse.column, mouse.row)
                {
                    match target {
                        ListClickTarget::Checkbox(index) => {
                            self.selected_index = index;
                            self.toggle_selected_todo(database)?;
                        }
                        ListClickTarget::Row(index) => self.selected_index = index,
                    }
                    self.ensure_selection_visible(self.visible_rows());
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_history_click(&mut self, database: &Database, area: Rect, y: u16) -> Result<()> {
        let overlay = centered_rect(area, 70, 16);
        let list_y = overlay.y.saturating_add(1);
        let index = usize::from(y.saturating_sub(list_y));
        if let Some(revision) = self.revisions.get(index) {
            self.revision = Some(revision.revision_number);
            self.overlay = None;
            self.reload(database)?;
        }
        Ok(())
    }

    fn handle_tick(&mut self, database: &mut Database) -> Result<()> {
        self.refresh_live_head(database)?;
        let Some(run) = self.active_run.clone() else {
            return Ok(());
        };
        if matches!(run.state, PomodoroState::Running)
            && remaining_seconds(&run, now_utc_timestamp()) == 0
        {
            database.complete_pomodoro(run.id, now_utc_timestamp())?;
            self.set_toast(String::from("Pomodoro completed"));
            self.reload(database)?;
        }
        Ok(())
    }

    fn refresh_live_head(&mut self, database: &Database) -> Result<()> {
        if self.revision.is_some() {
            return Ok(());
        }

        let latest = database.session_head_token(&self.session_slug)?;
        if self.head_token != Some(latest) {
            self.reload(database)?;
        }
        Ok(())
    }

    fn toggle_selected_todo(&mut self, database: &mut Database) -> Result<()> {
        let Some(todo) = self.current_todo() else {
            return Ok(());
        };
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return Ok(());
        }

        let next_status = match todo.status {
            TodoStatus::Open => TodoStatus::Done,
            TodoStatus::Done => TodoStatus::Open,
        };
        database.set_todo_status(
            todo.todo_id,
            Some(&self.session_slug),
            next_status,
            now_utc_timestamp(),
        )?;
        self.reload(database)
    }

    fn handle_pomodoro(&mut self, database: &mut Database, kind: PomodoroKind) -> Result<()> {
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return Ok(());
        }
        if let Some(run) = self.active_run.clone() {
            if run.session_id != self.snapshot().session.id {
                self.set_toast(String::from("Another session already has an active timer"));
                return Ok(());
            }
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
            let todo_id = self.current_todo().map(|todo| todo.todo_id);
            database.start_pomodoro(
                &self.session_slug,
                todo_id,
                kind,
                pomodoro_seconds(&self.config, kind),
                now_utc_timestamp(),
            )?;
        }
        self.reload(database)
    }

    fn cancel_active_pomodoro(&mut self, database: &mut Database) -> Result<()> {
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return Ok(());
        }
        if let Some(run) = self.current_session_run().cloned() {
            database.cancel_pomodoro(run.id, now_utc_timestamp())?;
            self.reload(database)?;
        }
        Ok(())
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>) {
        let snapshot = self.snapshot();
        let layout = self.layout_for_area(frame.area());
        frame.render_widget(self.top_bar(snapshot), layout.top_bar);
        frame.render_stateful_widget(
            self.todo_list(snapshot, layout.list.height),
            layout.list,
            &mut self.list_state(),
        );
        if let Some(details_area) = layout.details {
            frame.render_widget(self.details_panel(snapshot), details_area);
        }
        if let Some(pomodoro_area) = layout.pomodoro {
            frame.render_widget(self.pomodoro_panel(snapshot), pomodoro_area);
        }
        frame.render_widget(self.footer(layout.mode), layout.footer);

        if matches!(layout.mode, LayoutMode::Narrow)
            && matches!(self.overlay, Some(Overlay::Details))
        {
            let area = centered_rect(frame.area(), 52, 12);
            frame.render_widget(Clear, area);
            frame.render_widget(self.details_panel(snapshot), area);
        }
        if matches!(self.overlay, Some(Overlay::Help)) {
            let area = centered_rect(frame.area(), 54, 12);
            frame.render_widget(Clear, area);
            frame.render_widget(self.help_overlay(), area);
        }
        if matches!(self.overlay, Some(Overlay::History)) {
            let area = centered_rect(frame.area(), 70, 16);
            frame.render_widget(Clear, area);
            frame.render_stateful_widget(self.history_overlay(), area, &mut self.history_state());
        }
        if matches!(self.overlay, Some(Overlay::TodoEditor)) {
            let area = centered_rect(frame.area(), 60, 10);
            frame.render_widget(Clear, area);
            frame.render_widget(self.todo_editor_modal(), area);
        }
        if let Some(Overlay::DeleteTodo { title, .. }) = &self.overlay {
            let area = centered_rect(frame.area(), 60, 8);
            frame.render_widget(Clear, area);
            frame.render_widget(self.delete_todo_modal(title), area);
        }
        if let Some(Overlay::DeleteSession { slug, name }) = &self.overlay {
            let area = centered_rect(frame.area(), 60, 9);
            frame.render_widget(Clear, area);
            frame.render_widget(self.delete_session_modal(slug, name), area);
        }
        if let Some(toast) = &self.toast {
            let area = centered_rect(frame.area(), 50, 3);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(toast.message.clone())
                    .block(Block::default().borders(Borders::ALL).title("Notice"))
                    .style(
                        Style::default()
                            .fg(self.theme.fg_warning)
                            .bg(self.theme.bg_overlay),
                    ),
                area,
            );
        }
    }

    fn top_bar(&self, snapshot: &SessionSnapshot) -> Paragraph<'static> {
        let revision = self
            .revision
            .map_or_else(|| String::from("HEAD"), |value| format!("r{value}"));
        let mut lines = vec![Line::from(format!(
            "todui | {} ({}) | {revision}",
            snapshot.session.name, snapshot.session.slug
        ))];
        if self.is_read_only() {
            lines.push(Line::from(format!(
                "Viewing session {} @ r{} — {} — read-only",
                snapshot.session.slug,
                snapshot.revision.revision_number,
                format_full_local(snapshot.revision.created_at)
            )));
        } else if let Some(run) = self.current_session_run() {
            lines.push(Line::from(format!(
                "{} · {} remaining",
                run.kind.label(),
                format_duration(remaining_seconds(run, now_utc_timestamp()))
            )));
        }

        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("Session"))
            .style(self.theme.block_style())
    }

    fn top_bar_height(&self) -> u16 {
        if self.is_read_only() || self.current_session_run().is_some() {
            4
        } else {
            3
        }
    }

    fn layout_for_area(&self, area: Rect) -> crate::tui::layout::ScreenLayout {
        split_screen(
            area,
            self.medium_drawer_open,
            self.top_bar_height(),
            self.pomodoro_panel_height(self.snapshot()),
        )
    }

    fn todo_list(&self, snapshot: &SessionSnapshot, height: u16) -> Table<'static> {
        let visible_rows = height.saturating_sub(3).max(1) as usize;
        let rows = snapshot
            .todos
            .iter()
            .skip(self.scroll_offset)
            .take(visible_rows)
            .map(|todo| todo_table_row(todo, self.current_session_run()))
            .collect::<Vec<_>>();

        Table::new(
            rows,
            [
                Constraint::Length(3),
                Constraint::Fill(1),
                Constraint::Length(6),
                Constraint::Length(11),
            ],
        )
        .header(
            Row::new([
                Cell::from(""),
                Cell::from("Title"),
                Cell::from("Status"),
                Cell::from("Last Update"),
            ])
            .style(self.theme.block_style().add_modifier(Modifier::BOLD)),
        )
        .block(Block::default().borders(Borders::ALL).title("Todos"))
        .column_spacing(1)
        .row_highlight_style(self.theme.selected_style().add_modifier(Modifier::BOLD))
    }

    fn details_panel(&self, snapshot: &SessionSnapshot) -> Paragraph<'static> {
        let text = if let Some(todo) = self.current_todo() {
            format!(
                "title: {}\nstatus: {}\nnotes: {}\ncreated: {}\nupdated: {}\ncompleted: {}\nid: {}",
                todo.title,
                if todo.status == TodoStatus::Done {
                    "done"
                } else {
                    "open"
                },
                if todo.notes.trim().is_empty() {
                    "-"
                } else {
                    todo.notes.trim()
                },
                format_full_local(todo.created_at),
                format_full_local(todo.updated_at),
                todo.completed_at
                    .map(format_full_local)
                    .unwrap_or_else(|| String::from("-")),
                todo.todo_id
            )
        } else {
            format!("No todos in session {}", snapshot.session.slug)
        };

        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .style(self.theme.block_style())
    }

    fn pomodoro_panel_height(&self, snapshot: &SessionSnapshot) -> u16 {
        self.pomodoro_lines(snapshot).len() as u16 + 2
    }

    fn pomodoro_lines(&self, snapshot: &SessionSnapshot) -> Vec<Line<'static>> {
        let run = self.current_session_run();
        let mut lines = Vec::new();
        if self.is_read_only() {
            lines.push(Line::from(format!(
                "Focus runs up to this revision: {}",
                self.pomodoro_summary.completed_focus_runs
            )));
            lines.push(Line::from(format!(
                "Total focus time: {}",
                format_duration(self.pomodoro_summary.total_focus_seconds)
            )));
        } else if let Some(run) = run {
            lines.push(Line::from(format!(
                "{} · {} remaining",
                run.kind.label(),
                format_duration(remaining_seconds(run, now_utc_timestamp()))
            )));
            lines.push(Line::from(progress_bar(run, now_utc_timestamp())));
            lines.push(Line::from(format!(
                "Linked: {}",
                linked_title(run, snapshot)
            )));
            lines.push(Line::from("[p pause/resume] [c cancel]"));
        } else if self.active_run.is_some() {
            lines.push(Line::from("Another session has an active timer"));
        } else {
            lines.push(Line::from("No active run"));
            lines.push(Line::from("[p focus] [b short break] [B long break]"));
        }
        lines
    }

    fn pomodoro_panel(&self, snapshot: &SessionSnapshot) -> Paragraph<'static> {
        let _ = Gauge::default();

        Paragraph::new(self.pomodoro_lines(snapshot))
            .block(Block::default().borders(Borders::ALL).title("Pomodoro"))
            .style(self.theme.block_style())
    }

    fn footer(&self, layout_mode: LayoutMode) -> Paragraph<'static> {
        let text = if self.is_read_only() {
            "j/k move  H history  r return to head  Left/o overview  q quit"
        } else {
            match layout_mode {
                LayoutMode::Wide => {
                    "j/k move  n new  e edit  d del todo  D del session  space toggle  H history  p pomodoro  Left/o overview  q quit"
                }
                LayoutMode::Medium => {
                    "j/k move  n new  e edit  d del todo  D del session  space toggle  Tab drawer  H history  p pomodoro  Left/o overview  q quit"
                }
                LayoutMode::Narrow => {
                    "j/k move  n new  e edit  d del todo  D del session  space toggle  Enter details  H history  p pomodoro  Left/o overview  q quit"
                }
            }
        };
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title("Keys"))
            .style(self.theme.block_style())
    }

    fn help_overlay(&self) -> Paragraph<'static> {
        Paragraph::new(
            "Navigation: j/k, arrows, PageUp/PageDown\nNew todo: n\nEdit todo: e\nDelete todo: d\nDelete session: D\nToggle: space or x\nHistory: H\nPomodoro: p, b, B, c\nOverview: Left or o\nQuit: q or Esc",
        )
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .style(self.theme.block_style())
    }

    fn todo_editor_modal(&self) -> Paragraph<'_> {
        render_editor(
            &self.theme,
            EditorView {
                title: self.todo_editor_title(),
                primary_label: "Title",
                primary_value: &self.todo_editor.title,
                secondary_label: Some("Notes"),
                secondary_value: Some(&self.todo_editor.notes),
                focused_field: self.todo_editor.focused_field,
                error: self.todo_editor.error.as_deref(),
                footer_hint: self.todo_editor_footer_hint(),
            },
        )
    }

    fn delete_todo_modal(&self, title: &str) -> Paragraph<'static> {
        Paragraph::new(format!(
            "Delete todo?\n\n{title}\n\nEnter delete  Esc cancel"
        ))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Delete Todo"))
        .style(self.theme.block_style())
    }

    fn delete_session_modal(&self, slug: &str, name: &str) -> Paragraph<'static> {
        Paragraph::new(format!(
            "Delete session {name} ({slug})?\n\nThis permanently removes its todos, history, and pomodoro runs.\n\nEnter delete  Esc cancel"
        ))
        .wrap(Wrap { trim: false })
        .block(Block::default().borders(Borders::ALL).title("Delete Session"))
        .style(self.theme.block_style())
    }

    fn history_overlay(&self) -> List<'static> {
        let items = self
            .revisions
            .iter()
            .map(|revision| {
                ListItem::new(format!(
                    "r{}  {}  todo:{} done:{}  {}",
                    revision.revision_number,
                    format_full_local(revision.created_at),
                    revision.todo_count,
                    revision.done_count,
                    revision.reason
                ))
            })
            .collect::<Vec<_>>();
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("History"))
            .highlight_style(self.theme.selected_style().add_modifier(Modifier::BOLD))
    }

    fn current_todo(&self) -> Option<&RevisionTodo> {
        self.snapshot.as_ref()?.todos.get(self.selected_index)
    }

    fn current_session_run(&self) -> Option<&PomodoroRun> {
        self.active_run
            .as_ref()
            .filter(|run| run.session_id == self.snapshot().session.id)
    }

    fn snapshot(&self) -> &SessionSnapshot {
        self.snapshot.as_ref().expect("snapshot loaded")
    }

    fn visible_rows(&self) -> usize {
        8
    }

    fn is_read_only(&self) -> bool {
        matches!(self.snapshot().mode, RevisionMode::Historical(_))
    }

    fn reselect(&mut self, todo_id: Option<i64>) {
        if let Some(todo_id) = todo_id
            && let Some(index) = self
                .snapshot()
                .todos
                .iter()
                .position(|todo| todo.todo_id == todo_id)
        {
            self.selected_index = index;
        }
        self.selected_index = min(
            self.selected_index,
            self.snapshot().todos.len().saturating_sub(1),
        );
        self.ensure_selection_visible(self.visible_rows());
    }

    fn move_selection(&mut self, delta: isize, visible_rows: usize) {
        let todo_count = self.snapshot().todos.len();
        if todo_count == 0 {
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }
        if delta.is_negative() {
            self.selected_index = self.selected_index.saturating_sub(delta.unsigned_abs());
        } else {
            self.selected_index = min(
                self.selected_index + delta as usize,
                todo_count.saturating_sub(1),
            );
        }
        self.ensure_selection_visible(visible_rows);
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
        state.select(Some(self.selected_index.saturating_sub(self.scroll_offset)));
        state
    }

    fn history_state(&self) -> ListState {
        let mut state = ListState::default();
        state.select(Some(self.history_index));
        state
    }

    fn set_toast(&mut self, message: String) {
        self.toast = Some(ToastState {
            message,
            expires_at: Instant::now() + Duration::from_secs(2),
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

    fn open_todo_editor(&mut self) {
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return;
        }
        self.todo_editor = TodoEditorState {
            mode: TodoEditorMode::Create,
            focused_field: EditorField::Primary,
            ..TodoEditorState::default()
        };
        self.overlay = Some(Overlay::TodoEditor);
    }

    fn open_selected_todo_editor(&mut self) {
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return;
        }

        let Some((todo_id, title, notes)) = self
            .current_todo()
            .map(|todo| (todo.todo_id, todo.title.clone(), todo.notes.clone()))
        else {
            return;
        };
        self.todo_editor = TodoEditorState {
            mode: TodoEditorMode::Edit { todo_id },
            title,
            notes,
            focused_field: EditorField::Primary,
            error: None,
        };
        self.overlay = Some(Overlay::TodoEditor);
    }

    fn open_delete_todo(&mut self) {
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return;
        }
        let Some(todo) = self.current_todo() else {
            return;
        };
        self.overlay = Some(Overlay::DeleteTodo {
            id: todo.todo_id,
            title: todo.title.clone(),
        });
    }

    fn open_delete_session(&mut self) {
        if self.is_read_only() {
            self.set_toast(String::from("Historical revisions are read-only"));
            return;
        }
        let session = &self.snapshot().session;
        self.overlay = Some(Overlay::DeleteSession {
            slug: session.slug.clone(),
            name: session.name.clone(),
        });
    }

    fn close_todo_editor(&mut self) {
        self.overlay = None;
        self.todo_editor = TodoEditorState::default();
    }

    fn submit_todo_editor(&mut self, database: &mut Database) -> Result<()> {
        let title = self.todo_editor.title.trim();
        if title.is_empty() {
            self.todo_editor.error = Some(String::from("Todo title is required"));
            return Ok(());
        }

        let saved = match self.todo_editor.mode {
            TodoEditorMode::Create => database.add_todo(
                &self.session_slug,
                title,
                self.todo_editor.notes.trim(),
                now_utc_timestamp(),
            ),
            TodoEditorMode::Edit { todo_id } => database.update_todo(
                todo_id,
                Some(&self.session_slug),
                title,
                self.todo_editor.notes.trim(),
                now_utc_timestamp(),
            ),
        };

        let saved = match saved {
            Ok(todo) => todo,
            Err(error) => {
                self.todo_editor.error = Some(error.to_string());
                return Ok(());
            }
        };

        self.reload(database)?;
        self.reselect(Some(saved.id));
        let toast = match self.todo_editor.mode {
            TodoEditorMode::Create => String::from("Todo added"),
            TodoEditorMode::Edit { .. } => String::from("Todo updated"),
        };
        self.close_todo_editor();
        self.set_toast(toast);
        Ok(())
    }

    fn focused_todo_field(&mut self) -> &mut String {
        match self.todo_editor.focused_field {
            EditorField::Primary => &mut self.todo_editor.title,
            EditorField::Secondary => &mut self.todo_editor.notes,
        }
    }

    fn todo_editor_title(&self) -> &'static str {
        match self.todo_editor.mode {
            TodoEditorMode::Create => "New Todo",
            TodoEditorMode::Edit { .. } => "Edit Todo",
        }
    }

    fn todo_editor_footer_hint(&self) -> &'static str {
        match self.todo_editor.mode {
            TodoEditorMode::Create => "Tab next field  Enter create  Esc cancel",
            TodoEditorMode::Edit { .. } => "Tab next field  Enter save  Esc cancel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListClickTarget {
    Checkbox(usize),
    Row(usize),
}

fn list_click_target(
    list_area: Rect,
    scroll_offset: usize,
    x: u16,
    y: u16,
) -> Option<ListClickTarget> {
    let inner_x = list_area.x.saturating_add(1);
    let inner_y = list_area.y.saturating_add(1);
    if x < inner_x || y <= inner_y || y >= list_area.bottom().saturating_sub(1) {
        return None;
    }

    let row_index = scroll_offset + usize::from(y.saturating_sub(inner_y + 1));
    if x <= inner_x.saturating_add(2) {
        Some(ListClickTarget::Checkbox(row_index))
    } else {
        Some(ListClickTarget::Row(row_index))
    }
}

fn pomodoro_seconds(config: &Config, kind: PomodoroKind) -> i64 {
    match kind {
        PomodoroKind::Focus => i64::from(config.pomodoro.focus_minutes) * 60,
        PomodoroKind::ShortBreak => i64::from(config.pomodoro.short_break_minutes) * 60,
        PomodoroKind::LongBreak => i64::from(config.pomodoro.long_break_minutes) * 60,
    }
}

fn format_duration(seconds: i64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn linked_title(run: &PomodoroRun, snapshot: &SessionSnapshot) -> String {
    run.todo_id
        .and_then(|todo_id| {
            snapshot
                .todos
                .iter()
                .find(|todo| todo.todo_id == todo_id)
                .map(|todo| todo.title.clone())
        })
        .unwrap_or_else(|| String::from("session-only"))
}

fn todo_table_row(todo: &RevisionTodo, run: Option<&PomodoroRun>) -> Row<'static> {
    Row::new([
        Cell::from(todo_checkbox(todo)),
        Cell::from(todo.title.clone()),
        Cell::from(todo_status_label(todo, run)),
        Cell::from(todo_time_label(todo)),
    ])
}

fn todo_checkbox(todo: &RevisionTodo) -> &'static str {
    match todo.status {
        TodoStatus::Open => "[ ]",
        TodoStatus::Done => "[x]",
    }
}

fn todo_status_label(todo: &RevisionTodo, run: Option<&PomodoroRun>) -> &'static str {
    if run.is_some_and(|active| {
        active.todo_id == Some(todo.todo_id) && matches!(active.kind, PomodoroKind::Focus)
    }) {
        "FOCUS"
    } else {
        match todo.status {
            TodoStatus::Open => "open",
            TodoStatus::Done => "done",
        }
    }
}

fn todo_time_label(todo: &RevisionTodo) -> String {
    let timestamp = match todo.status {
        TodoStatus::Open => todo.created_at,
        TodoStatus::Done => todo.completed_at.unwrap_or(todo.updated_at),
    };
    format_month_day_local(timestamp)
}

fn key_matches_binding(key: &KeyEvent, bindings: &[String]) -> bool {
    bindings.iter().any(|binding| match binding.as_str() {
        "up" => matches!(key.code, KeyCode::Up),
        "down" => matches!(key.code, KeyCode::Down),
        "space" => matches!(key.code, KeyCode::Char(' ')),
        value if value.len() == 1 => {
            matches!(key.code, KeyCode::Char(character) if value.starts_with(character))
        }
        _ => false,
    })
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use crossterm::event::{
        KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use super::{
        ListClickTarget, Overlay, SessionExit, SessionScreen, format_duration, key_matches_binding,
        list_click_target, progress_bar, todo_status_label, todo_time_label,
    };
    use crate::config::Config;
    use crate::db::Database;
    use crate::domain::pomodoro::{PomodoroKind, PomodoroState};
    use crate::domain::revision::RevisionMode;
    use crate::tui::layout::split_screen;

    #[test]
    fn identifies_checkbox_and_row_click_targets() {
        let area = Rect::new(0, 0, 40, 10);
        assert_eq!(list_click_target(area, 0, 1, 1), None);
        assert_eq!(
            list_click_target(area, 0, 1, 2),
            Some(ListClickTarget::Checkbox(0))
        );
        assert_eq!(
            list_click_target(area, 0, 6, 3),
            Some(ListClickTarget::Row(1))
        );
    }

    #[test]
    fn formats_duration_mm_ss() {
        assert_eq!(format_duration(65), "01:05");
    }

    #[test]
    fn key_binding_matches_defaults_and_custom_values() {
        let key = key(KeyCode::Char('x'));
        assert!(key_matches_binding(
            &key,
            &[String::from("space"), String::from("x")]
        ));
        assert!(!key_matches_binding(&key, &[String::from("p")]));
    }

    #[test]
    fn progress_bar_renders_percentage() {
        let run = crate::domain::pomodoro::PomodoroRun {
            id: 1,
            session_id: 1,
            todo_id: None,
            kind: PomodoroKind::Focus,
            state: PomodoroState::Running,
            planned_seconds: 100,
            started_at: 0,
            paused_at: None,
            accumulated_pause: 0,
            ended_at: None,
            updated_at: 0,
        };
        assert!(progress_bar(&run, 50).contains("50%"));
    }

    #[test]
    fn screen_handles_navigation_toggle_history_and_read_only_paths() {
        let (_directory, mut database, mut screen) = seeded_screen();
        assert!(
            screen
                .handle_key(&mut database, key(KeyCode::Char('?')))
                .unwrap()
                .is_none()
        );
        assert!(matches!(screen.overlay, Some(Overlay::Help)));
        screen.handle_key(&mut database, key(KeyCode::Esc)).unwrap();
        assert!(screen.overlay.is_none());

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        assert_eq!(screen.selected_index, 1);
        screen.handle_key(&mut database, key(KeyCode::Up)).unwrap();
        assert_eq!(screen.selected_index, 0);
        screen.handle_key(&mut database, key(KeyCode::End)).unwrap();
        assert_eq!(screen.selected_index, 1);
        screen
            .handle_key(&mut database, key(KeyCode::Home))
            .unwrap();
        assert_eq!(screen.selected_index, 0);
        screen
            .handle_key(&mut database, key(KeyCode::PageDown))
            .unwrap();
        assert_eq!(screen.selected_index, 1);
        screen
            .handle_key(&mut database, key(KeyCode::Home))
            .unwrap();

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        assert!(render_buffer(&screen, 120, 24).contains("New Todo"));
        for character in "Outline modal flow".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for character in "include notes".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(screen.snapshot().todos.len(), 3);
        assert_eq!(
            screen.current_todo().expect("selected").title,
            "Outline modal flow"
        );
        assert_eq!(
            screen.current_todo().expect("selected").notes,
            "include notes"
        );

        screen
            .handle_key(&mut database, key(KeyCode::Char('x')))
            .unwrap();
        assert_eq!(
            screen.snapshot().todos[2].status,
            crate::domain::todo::TodoStatus::Done
        );

        screen
            .handle_key(&mut database, key(KeyCode::Char('H')))
            .unwrap();
        assert!(matches!(screen.overlay, Some(Overlay::History)));
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(matches!(
            screen.snapshot().mode,
            RevisionMode::Historical(_)
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('x')))
            .unwrap();
        assert!(screen.toast.as_ref().unwrap().message.contains("read-only"));

        screen
            .handle_key(&mut database, key(KeyCode::Char('r')))
            .unwrap();
        assert!(matches!(screen.snapshot().mode, RevisionMode::Head));
        screen
            .handle_key(&mut database, key(KeyCode::Home))
            .unwrap();

        screen
            .handle_key(&mut database, key(KeyCode::Char('e')))
            .unwrap();
        assert!(render_buffer(&screen, 120, 24).contains("Edit Todo"));
        for _ in 0.."Draft spec".len() {
            screen
                .handle_key(&mut database, key(KeyCode::Backspace))
                .unwrap();
        }
        for character in "Draft polished spec".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for _ in 0.."cover db".len() {
            screen
                .handle_key(&mut database, key(KeyCode::Backspace))
                .unwrap();
        }
        for character in "cover db and tui".chars() {
            screen
                .handle_key(&mut database, key(KeyCode::Char(character)))
                .unwrap();
        }
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(screen.snapshot().todos[0].title, "Draft polished spec");
        assert_eq!(screen.snapshot().todos[0].notes, "cover db and tui");

        assert_eq!(
            screen
                .handle_key(&mut database, key(KeyCode::Char('o')))
                .unwrap(),
            Some(SessionExit::Overview)
        );
    }

    #[test]
    fn screen_blocks_blank_todo_title_and_read_only_creation() {
        let (_directory, mut database, mut screen) = seeded_screen();

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(render_buffer(&screen, 120, 24).contains("Todo title is required"));
        assert_eq!(screen.snapshot().todos.len(), 2);

        screen.handle_key(&mut database, key(KeyCode::Esc)).unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Char('H')))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert!(matches!(
            screen.snapshot().mode,
            RevisionMode::Historical(_)
        ));
        let revision_todo_count = screen.snapshot().todos.len();

        screen
            .handle_key(&mut database, key(KeyCode::Char('n')))
            .unwrap();
        assert!(screen.toast.as_ref().unwrap().message.contains("read-only"));
        assert_eq!(screen.snapshot().todos.len(), revision_todo_count);

        screen
            .handle_key(&mut database, key(KeyCode::Char('e')))
            .unwrap();
        assert!(screen.toast.as_ref().unwrap().message.contains("read-only"));
    }

    #[test]
    fn screen_left_arrow_returns_to_overview_without_overriding_overlays() {
        let (_directory, mut database, mut screen) = seeded_screen();

        assert_eq!(
            screen
                .handle_key(&mut database, key(KeyCode::Left))
                .unwrap(),
            Some(SessionExit::Overview)
        );

        screen.overlay = Some(Overlay::Help);
        assert!(
            screen
                .handle_key(&mut database, key(KeyCode::Left))
                .unwrap()
                .is_none()
        );
        assert!(matches!(screen.overlay, Some(Overlay::Help)));
    }

    #[test]
    fn screen_handles_mouse_history_and_pomodoro_controls() {
        let (_directory, mut database, mut screen) = seeded_screen();
        let area = Rect::new(0, 0, 120, 24);

        screen
            .handle_mouse(
                &mut database,
                area,
                mouse(MouseEventKind::Down(MouseButton::Left), 6, 5),
            )
            .unwrap();
        assert_eq!(screen.selected_index, 0);

        screen
            .handle_mouse(
                &mut database,
                area,
                mouse(MouseEventKind::Down(MouseButton::Left), 1, 5),
            )
            .unwrap();
        assert_eq!(
            screen.snapshot().todos[0].status,
            crate::domain::todo::TodoStatus::Done
        );

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(screen.current_session_run().is_some());

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.current_session_run().unwrap().state,
            PomodoroState::Paused
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.current_session_run().unwrap().state,
            PomodoroState::Running
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('c')))
            .unwrap();
        assert!(screen.current_session_run().is_none());

        screen.overlay = Some(Overlay::History);
        screen
            .handle_mouse(
                &mut database,
                area,
                mouse(MouseEventKind::Down(MouseButton::Left), 5, 6),
            )
            .unwrap();
        assert!(matches!(
            screen.snapshot().mode,
            RevisionMode::Historical(_)
        ));
    }

    #[test]
    fn screen_confirms_and_deletes_selected_todo() {
        let (_directory, mut database, mut screen) = seeded_screen();

        screen
            .handle_key(&mut database, key(KeyCode::Char('d')))
            .unwrap();
        assert!(render_buffer(&screen, 120, 24).contains("Delete Todo"));

        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(screen.snapshot().todos.len(), 1);
        assert_eq!(screen.snapshot().todos[0].title, "Review bindings");
        assert!(
            screen
                .toast
                .as_ref()
                .unwrap()
                .message
                .contains("Todo deleted")
        );
    }

    #[test]
    fn screen_confirms_and_deletes_current_session() {
        let (_directory, mut database, mut screen) = seeded_screen();

        screen
            .handle_key(&mut database, key(KeyCode::Char('D')))
            .unwrap();
        assert!(render_buffer(&screen, 120, 24).contains("Delete Session"));

        let exit = screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();
        assert_eq!(exit, Some(SessionExit::Overview));
        assert!(database.get_session_by_slug("writing-sprint").is_err());
    }

    #[test]
    fn screen_blocks_delete_in_read_only_revision() {
        let (_directory, mut database, mut screen) = seeded_screen();

        screen
            .handle_key(&mut database, key(KeyCode::Char('H')))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Enter))
            .unwrap();

        screen
            .handle_key(&mut database, key(KeyCode::Char('d')))
            .unwrap();
        assert!(screen.toast.as_ref().unwrap().message.contains("read-only"));

        screen
            .handle_key(&mut database, key(KeyCode::Char('D')))
            .unwrap();
        assert!(screen.toast.as_ref().unwrap().message.contains("read-only"));
    }

    #[test]
    fn screen_tick_completes_timer_and_toast_expires() {
        let (_directory, mut database, mut screen) = seeded_screen();
        let run = database
            .start_pomodoro(&screen.session_slug, None, PomodoroKind::Focus, 1, 0)
            .expect("run");
        database.complete_pomodoro(run.id, 1).expect("complete");
        screen.reload(&database).unwrap();
        screen.active_run = Some(crate::domain::pomodoro::PomodoroRun {
            state: PomodoroState::Running,
            started_at: 0,
            planned_seconds: 0,
            ..run
        });
        screen.handle_tick(&mut database).unwrap();
        assert!(screen.toast.as_ref().unwrap().message.contains("completed"));
        assert!(screen.take_pending_bell());
        assert!(!screen.take_pending_bell());

        screen.toast.as_mut().unwrap().expires_at = Instant::now() - Duration::from_secs(1);
        screen.expire_toast();
        assert!(screen.toast.is_none());
    }

    #[test]
    fn screen_tick_skips_bell_when_completion_notifications_are_disabled() {
        let (_directory, mut database, mut screen) = seeded_screen();
        screen.config.pomodoro.notify_on_complete = false;

        let run = database
            .start_pomodoro(&screen.session_slug, None, PomodoroKind::ShortBreak, 1, 0)
            .expect("run");
        database.complete_pomodoro(run.id, 1).expect("complete");
        screen.reload(&database).unwrap();
        screen.active_run = Some(crate::domain::pomodoro::PomodoroRun {
            state: PomodoroState::Running,
            started_at: 0,
            planned_seconds: 0,
            ..run
        });

        screen.handle_tick(&mut database).unwrap();

        assert!(screen.toast.as_ref().unwrap().message.contains("completed"));
        assert!(!screen.take_pending_bell());
    }

    #[test]
    fn screen_tick_refreshes_head_after_external_todo_add() {
        let (_directory, mut database, mut screen, database_path) = seeded_screen_with_path();
        let mut external = Database::open(&database_path).expect("external database");

        external
            .add_todo(&screen.session_slug, "Ship live refresh", "", 1_711_275_900)
            .expect("external todo");
        screen.handle_tick(&mut database).expect("tick");

        let titles = screen
            .snapshot()
            .todos
            .iter()
            .map(|todo| todo.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            vec!["Draft spec", "Review bindings", "Ship live refresh"]
        );
    }

    #[test]
    fn screen_tick_keeps_historical_revision_frozen() {
        let (_directory, mut database, mut screen, database_path) = seeded_screen_with_path();
        let mut external = Database::open(&database_path).expect("external database");
        let revision = screen.snapshot().revision.revision_number;

        screen.revision = Some(revision);
        screen.reload(&database).expect("load historical revision");
        external
            .add_todo(
                &screen.session_slug,
                "Should stay hidden",
                "",
                1_711_275_900,
            )
            .expect("external todo");

        screen.handle_tick(&mut database).expect("tick");

        assert_eq!(screen.snapshot().todos.len(), 2);
        assert!(
            screen
                .snapshot()
                .todos
                .iter()
                .all(|todo| todo.title != "Should stay hidden")
        );
    }

    #[test]
    fn screen_tick_ignores_unrelated_session_creation() {
        let (_directory, mut database, mut screen, database_path) = seeded_screen_with_path();
        let mut external = Database::open(&database_path).expect("external database");

        external
            .create_session("Reading Sprint", None, None, 1_711_275_900)
            .expect("external session");
        screen.handle_tick(&mut database).expect("tick");

        assert_eq!(screen.session_slug, "writing-sprint");
        assert_eq!(screen.snapshot().todos.len(), 2);
    }

    #[test]
    fn render_covers_wide_medium_narrow_and_overlay_states() {
        let (_directory, _database, mut screen) = seeded_screen();

        let wide = render_buffer(&screen, 120, 24);
        assert!(wide.contains("Writing Sprint"));
        assert!(wide.contains("Pomodoro"));
        assert!(wide.contains("Status"));
        assert!(wide.contains("Last Update"));
        let wide_lines = wide.lines().collect::<Vec<_>>();
        assert!(wide_lines[2].starts_with("└"));
        assert!(wide_lines[3].contains("Todos"));
        assert!(wide.contains("e edit"));
        assert!(wide.contains("Left/o overview"));

        screen.medium_drawer_open = true;
        let medium = render_buffer(&screen, 80, 24);
        assert!(medium.contains("Keys"));

        screen.overlay = Some(Overlay::Details);
        let narrow = render_buffer(&screen, 60, 24);
        assert!(narrow.contains("Details"));

        screen.overlay = Some(Overlay::History);
        let history = render_buffer(&screen, 120, 24);
        assert!(history.contains("History"));

        screen.overlay = Some(Overlay::Help);
        let help = render_buffer(&screen, 120, 24);
        assert!(help.contains("Navigation"));
        assert!(help.contains("Overview: Left or o"));

        screen.overlay = Some(Overlay::TodoEditor);
        let editor = render_buffer(&screen, 120, 24);
        assert!(editor.contains("New Todo"));
        assert!(editor.contains("Title"));

        screen.toast = Some(super::ToastState {
            message: String::from("hello"),
            expires_at: Instant::now() + Duration::from_secs(1),
        });
        let toast = render_buffer(&screen, 120, 24);
        assert!(toast.contains("Notice"));
    }

    #[test]
    fn idle_pomodoro_panel_has_no_blank_row_before_bottom_border() {
        let (_directory, _database, mut screen) = seeded_screen();
        let wide_buffer = render_test_buffer(&screen, 120, 24);
        let wide_layout = split_screen(
            Rect::new(0, 0, 120, 24),
            screen.medium_drawer_open,
            screen.top_bar_height(),
            screen.pomodoro_panel_height(screen.snapshot()),
        );
        let wide_pomodoro = wide_layout.pomodoro.expect("wide pomodoro");
        assert_eq!(wide_pomodoro.height, 4);
        assert_eq!(
            wide_buffer[(
                wide_pomodoro.x + 1,
                wide_pomodoro.y + wide_pomodoro.height - 1
            )]
                .symbol(),
            "─"
        );

        screen.medium_drawer_open = true;
        let medium_buffer = render_test_buffer(&screen, 80, 24);
        let medium_layout = split_screen(
            Rect::new(0, 0, 80, 24),
            screen.medium_drawer_open,
            screen.top_bar_height(),
            screen.pomodoro_panel_height(screen.snapshot()),
        );
        let medium_pomodoro = medium_layout.pomodoro.expect("medium pomodoro");
        assert_eq!(medium_pomodoro.height, 4);
        assert_eq!(
            medium_buffer[(
                medium_pomodoro.x + 1,
                medium_pomodoro.y + medium_pomodoro.height - 1
            )]
                .symbol(),
            "─"
        );
    }

    #[test]
    fn todo_row_retains_stateful_timestamp_semantics() {
        let (_directory, _database, screen) = seeded_screen();
        let open = &screen.snapshot().todos[0];

        assert_eq!(todo_status_label(open, None), "open");
        assert_eq!(
            todo_time_label(open),
            crate::timestamp::format_month_day_local(open.created_at)
        );

        let open_time = screen.snapshot().todos[1].created_at;
        let done = crate::domain::revision::RevisionTodo {
            status: crate::domain::todo::TodoStatus::Done,
            completed_at: Some(open_time + 60),
            ..screen.snapshot().todos[1].clone()
        };
        assert_eq!(todo_status_label(&done, None), "done");
        assert_eq!(
            todo_time_label(&done),
            crate::timestamp::format_month_day_local(open_time + 60)
        );
    }

    fn seeded_screen() -> (tempfile::TempDir, Database, SessionScreen) {
        let (directory, database, screen, _) = seeded_screen_with_path();
        (directory, database, screen)
    }

    fn seeded_screen_with_path() -> (tempfile::TempDir, Database, SessionScreen, PathBuf) {
        let (directory, mut database) = Database::open_temp().expect("database");
        let database_path = directory.path().join("todui.db");
        let session = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");
        database
            .add_todo(&session.slug, "Draft spec", "cover db", 1_711_275_700)
            .expect("todo");
        database
            .add_todo(&session.slug, "Review bindings", "", 1_711_275_800)
            .expect("todo");

        let mut screen = SessionScreen::new(session.slug, None, Config::default());
        screen.reload(&database).expect("reload");
        (directory, database, screen, database_path)
    }

    fn render_buffer(screen: &SessionScreen, width: u16, height: u16) -> String {
        buffer_to_string(&render_test_buffer(screen, width, height))
    }

    fn render_test_buffer(screen: &SessionScreen, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| screen.render(frame)).expect("draw");
        terminal.backend().buffer().clone()
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
