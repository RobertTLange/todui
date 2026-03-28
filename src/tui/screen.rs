use std::cmp::min;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::config::Config;
use crate::db::Database;
use crate::domain::pomodoro::{PomodoroKind, PomodoroRun, PomodoroState, remaining_seconds};
use crate::domain::revision::{RevisionMode, RevisionSummary, RevisionTodo, SessionSnapshot};
use crate::domain::session::SessionHeadToken;
use crate::domain::todo::TodoStatus;
use crate::error::Result;
use crate::timestamp::{format_full_local, now_utc_timestamp};
use crate::tui::layout::{centered_rect, split_screen};
use crate::tui::terminal::AppTerminal;
use crate::tui::theme::{SelectionTone, SurfaceTone, TextTone, Theme};
use crate::tui::widgets::editor::{EditorField, EditorView, render_editor};
use crate::tui::widgets::pomodoro::{active_footer, active_footer_height};
use crate::tui::widgets::todo_list::{
    GroupedTodos, TodoClickTarget, TodoSection, section_state, section_visible_rows,
    split_todo_list_area, todo_click_target, todo_section_table,
};

const EVENT_POLL_MS: u64 = 250;

pub fn run(
    database: &mut Database,
    config: &Config,
    session_name: Option<String>,
    revision: Option<u32>,
) -> Result<()> {
    super::run(
        database,
        config,
        super::TuiRoute::Session {
            session_name,
            revision,
        },
    )
}

pub(crate) fn run_in_terminal(
    terminal: &mut AppTerminal,
    database: &mut Database,
    config: &Config,
    session_name: Option<String>,
    revision: Option<u32>,
) -> Result<SessionExit> {
    let resolved_name = database.resolve_session_name(session_name.as_deref())?;
    database.mark_session_opened(&resolved_name, now_utc_timestamp())?;

    let mut screen = SessionScreen::new(resolved_name, revision, config.clone());
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
                    if let Some(exit) =
                        screen.handle_key_in_area(database, key_event, terminal.size()?.into())?
                    {
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
            if screen.take_pending_bell() {
                let _ = super::terminal::ring_terminal(terminal);
            }
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
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TodoEditorState {
    mode: TodoEditorMode,
    title: String,
    notes: String,
    repo: String,
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
    session_name: String,
    revision: Option<u32>,
    snapshot: Option<SessionSnapshot>,
    revisions: Vec<RevisionSummary>,
    active_run: Option<PomodoroRun>,
    head_token: Option<SessionHeadToken>,
    selected_index: usize,
    open_scroll_offset: usize,
    completed_scroll_offset: usize,
    history_index: usize,
    overlay: Option<Overlay>,
    todo_editor: TodoEditorState,
    toast: Option<ToastState>,
    pending_completion_bell: bool,
    viewport_area: Rect,
    theme: Theme,
    config: Config,
}

impl SessionScreen {
    fn new(session_name: String, revision: Option<u32>, config: Config) -> Self {
        Self {
            session_name,
            revision,
            snapshot: None,
            revisions: Vec::new(),
            active_run: None,
            head_token: None,
            selected_index: 0,
            open_scroll_offset: 0,
            completed_scroll_offset: 0,
            history_index: 0,
            overlay: None,
            todo_editor: TodoEditorState::default(),
            toast: None,
            pending_completion_bell: false,
            viewport_area: Rect::new(0, 0, 120, 24),
            theme: Theme::from_config(&config),
            config,
        }
    }

    fn reload(&mut self, database: &Database) -> Result<()> {
        let selected_todo_id = self.current_todo().map(|todo| todo.todo_id);
        let snapshot = database.load_snapshot(&self.session_name, self.revision)?;
        self.revisions = database.list_revisions(&self.session_name)?;
        if self.is_read_only_snapshot(&snapshot) {
            self.active_run = None;
        } else {
            self.active_run = database.get_active_pomodoro()?;
        }
        self.head_token = Some(database.session_head_token(&self.session_name)?);
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

    #[cfg(test)]
    fn handle_key(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
    ) -> Result<Option<SessionExit>> {
        self.handle_key_in_area(database, key, Rect::new(0, 0, 120, 24))
    }

    fn handle_key_in_area(
        &mut self,
        database: &mut Database,
        key: KeyEvent,
        area: Rect,
    ) -> Result<Option<SessionExit>> {
        self.viewport_area = area;

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Ok(Some(SessionExit::Quit));
        }

        match self.overlay {
            Some(Overlay::History) => return self.handle_history_key(database, key),
            Some(Overlay::Help) => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h')
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
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::Left
                ) {
                    self.overlay = None;
                }
                return Ok(None);
            }
            None => {}
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(Some(SessionExit::Quit)),
            KeyCode::Left | KeyCode::Char('o') => Ok(Some(SessionExit::Overview)),
            KeyCode::Right | KeyCode::Char('i') => {
                self.open_details();
                Ok(None)
            }
            KeyCode::Char('h') => {
                self.overlay = Some(Overlay::Help);
                Ok(None)
            }
            KeyCode::Up | KeyCode::Char('k')
                if matches!(key.code, KeyCode::Up)
                    || key_matches_binding(&key, &self.config.keys.up) =>
            {
                self.move_selection(-1);
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j')
                if matches!(key.code, KeyCode::Down)
                    || key_matches_binding(&key, &self.config.keys.down) =>
            {
                self.move_selection(1);
                Ok(None)
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected_index = 0;
                self.open_scroll_offset = 0;
                self.completed_scroll_offset = 0;
                self.ensure_selection_visible();
                Ok(None)
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected_index = self.grouped_todos().len().saturating_sub(1);
                self.ensure_selection_visible();
                Ok(None)
            }
            KeyCode::PageUp => {
                self.move_selection(-(self.page_size() as isize));
                Ok(None)
            }
            KeyCode::PageDown => {
                self.move_selection(self.page_size() as isize);
                Ok(None)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-(self.page_size() as isize));
                Ok(None)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(self.page_size() as isize);
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
            KeyCode::Enter => Ok(None),
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
                    EditorField::Secondary => EditorField::Tertiary,
                    EditorField::Tertiary => EditorField::Primary,
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
                        database.delete_todo(id, Some(&self.session_name), now_utc_timestamp())?;
                        self.reload(database)?;
                        self.overlay = None;
                        self.set_toast(String::from("Todo deleted"), ToastTone::Danger);
                        Ok(None)
                    }
                    Some(Overlay::DeleteSession { name }) => {
                        database.delete_session(&name)?;
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
        self.viewport_area = area;

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
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::ScrollDown => self.move_selection(1),
            MouseEventKind::Down(MouseButton::Left) => {
                if matches!(self.overlay, Some(Overlay::History)) {
                    self.handle_history_click(database, area, mouse.row)?;
                    return Ok(());
                }
                let layout = self.layout_for_area(area);
                let list_areas = split_todo_list_area(layout.list);
                if let Some(target) = todo_click_target(
                    list_areas,
                    self.open_scroll_offset,
                    self.completed_scroll_offset,
                    mouse.column,
                    mouse.row,
                ) {
                    let grouped = self.grouped_todos();
                    let flat_index = match target {
                        TodoClickTarget::Checkbox { section, row }
                        | TodoClickTarget::Row { section, row } => {
                            grouped.flat_index_for_section_row(section, row)
                        }
                    };
                    if let Some(flat_index) = flat_index {
                        self.selected_index = flat_index;
                        if matches!(target, TodoClickTarget::Checkbox { .. }) {
                            self.toggle_selected_todo(database)?;
                        } else {
                            self.ensure_selection_visible();
                        }
                    }
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
            if self.config.pomodoro.notify_on_complete {
                self.pending_completion_bell = true;
            }
            self.set_toast(String::from("Pomodoro completed"), ToastTone::Success);
            self.reload(database)?;
        }
        Ok(())
    }

    fn refresh_live_head(&mut self, database: &Database) -> Result<()> {
        if self.revision.is_some() {
            return Ok(());
        }

        let latest = database.session_head_token(&self.session_name)?;
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
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
            return Ok(());
        }

        let next_status = match todo.status {
            TodoStatus::Open => TodoStatus::Done,
            TodoStatus::Done => TodoStatus::Open,
        };
        database.set_todo_status(
            todo.todo_id,
            Some(&self.session_name),
            next_status,
            now_utc_timestamp(),
        )?;
        self.reload(database)
    }

    fn handle_pomodoro(&mut self, database: &mut Database, kind: PomodoroKind) -> Result<()> {
        if self.is_read_only() {
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
            return Ok(());
        }
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
        if self.is_read_only() {
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
            return Ok(());
        }
        if let Some(run) = self.active_run.clone() {
            database.cancel_pomodoro(run.id, now_utc_timestamp())?;
            self.reload(database)?;
        }
        Ok(())
    }

    fn render(&self, frame: &mut ratatui::Frame<'_>) {
        let snapshot = self.snapshot();
        let layout = self.layout_for_area(frame.area());
        let grouped = self.grouped_todos();
        let list_areas = split_todo_list_area(layout.list);
        frame.render_widget(Block::default().style(self.theme.app_style()), frame.area());
        frame.render_widget(self.top_bar(snapshot), layout.top_bar);
        frame.render_stateful_widget(
            todo_section_table(
                "Open",
                TodoSection::Open,
                grouped.open(),
                self.open_scroll_offset,
                section_visible_rows(list_areas.open),
                self.active_run.as_ref(),
                &self.theme,
            ),
            list_areas.open,
            &mut self.open_list_state(),
        );
        frame.render_stateful_widget(
            todo_section_table(
                "Completed",
                TodoSection::Completed,
                grouped.completed(),
                self.completed_scroll_offset,
                section_visible_rows(list_areas.completed),
                self.active_run.as_ref(),
                &self.theme,
            ),
            list_areas.completed,
            &mut self.completed_list_state(),
        );
        if let Some(footer_area) = layout.footer
            && let Some(run) = self.active_run.as_ref()
        {
            frame.render_widget(
                active_footer(&self.theme, run, now_utc_timestamp()),
                footer_area,
            );
        }

        if matches!(self.overlay, Some(Overlay::Details)) {
            let area = centered_rect(frame.area(), 60, 12);
            frame.render_widget(Clear, area);
            frame.render_widget(self.details_panel(snapshot), area);
        }
        if matches!(self.overlay, Some(Overlay::Help)) {
            let area = centered_rect(frame.area(), 60, 16);
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
        if let Some(Overlay::DeleteSession { name }) = &self.overlay {
            let area = centered_rect(frame.area(), 60, 9);
            frame.render_widget(Clear, area);
            frame.render_widget(self.delete_session_modal(name), area);
        }
        if let Some(toast) = &self.toast {
            let area = centered_rect(frame.area(), 50, 3);
            frame.render_widget(Clear, area);
            let surface_tone = match toast.tone {
                ToastTone::Success => SurfaceTone::Overlay,
                ToastTone::Warning => SurfaceTone::Notice,
                ToastTone::Danger => SurfaceTone::Danger,
            };
            let text_tone = match toast.tone {
                ToastTone::Success => TextTone::Focus,
                ToastTone::Warning => TextTone::Warning,
                ToastTone::Danger => TextTone::Danger,
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

    fn top_bar(&self, snapshot: &SessionSnapshot) -> Paragraph<'static> {
        let revision = self
            .revision
            .map_or_else(|| String::from("HEAD"), |value| format!("r{value}"));
        let mut lines = vec![Line::from(format!(
            "todui | {} | {revision} | h = help",
            snapshot.session.name
        ))];
        if self.is_read_only() {
            lines.push(Line::styled(
                format!(
                    "Viewing session {} @ r{} — {} — read-only",
                    snapshot.session.name,
                    snapshot.revision.revision_number,
                    format_full_local(snapshot.revision.created_at)
                ),
                self.theme.text_style(TextTone::Danger),
            ));
        }

        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Session")
                    .style(self.theme.surface_style(SurfaceTone::Neutral))
                    .border_style(self.theme.surface_border_style(SurfaceTone::Open))
                    .title_style(self.theme.surface_title_style(SurfaceTone::Open)),
            )
            .style(self.theme.surface_style(SurfaceTone::Neutral))
    }

    fn top_bar_height(&self) -> u16 {
        if self.is_read_only() { 4 } else { 3 }
    }

    fn layout_for_area(&self, area: Rect) -> crate::tui::layout::ScreenLayout {
        split_screen(area, self.top_bar_height(), self.active_footer_height())
    }

    fn details_panel(&self, snapshot: &SessionSnapshot) -> Paragraph<'static> {
        let text = if let Some(todo) = self.current_todo() {
            let (effective_repo, repo_source) = self.current_todo_repo_details(todo);
            format!(
                "title: {}\nstatus: {}\nrepo: {}\nrepo source: {}\nnotes: {}\ncreated: {}\nupdated: {}\ncompleted: {}\nid: {}",
                todo.title,
                if todo.status == TodoStatus::Done {
                    "done"
                } else {
                    "open"
                },
                effective_repo.unwrap_or_else(|| String::from("-")),
                repo_source,
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
            format!("No todos in session {}", snapshot.session.name)
        };

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

    fn help_overlay(&self) -> Paragraph<'static> {
        Paragraph::new(
            "Navigation: j/k, arrows, PageUp/PageDown\nHelp: h\nDetails: i or Right\nNew todo: n\nEdit todo: e\nDelete todo: d\nDelete session: D\nToggle: space or x\nHistory: H\nPomodoro: p start/pause/resume focus\nBreaks: b short break, B long break\nCancel timer: c\nOverview: Left or o\nQuit: q or Esc",
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

    fn todo_editor_modal(&self) -> Paragraph<'_> {
        render_editor(
            &self.theme,
            EditorView {
                title: self.todo_editor_title(),
                primary_label: "Title",
                primary_value: &self.todo_editor.title,
                secondary_label: Some("Notes"),
                secondary_value: Some(&self.todo_editor.notes),
                tertiary_label: Some("Repo"),
                tertiary_value: Some(&self.todo_editor.repo),
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Delete Todo")
                .style(self.theme.surface_style(SurfaceTone::Danger))
                .border_style(self.theme.surface_border_style(SurfaceTone::Danger))
                .title_style(self.theme.surface_title_style(SurfaceTone::Danger)),
        )
        .style(self.theme.surface_style(SurfaceTone::Danger))
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
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("History")
                    .style(self.theme.surface_style(SurfaceTone::Overlay))
                    .border_style(self.theme.surface_border_style(SurfaceTone::History))
                    .title_style(self.theme.surface_title_style(SurfaceTone::History)),
            )
            .highlight_style(self.theme.selection_style(SelectionTone::History))
    }

    fn current_todo(&self) -> Option<&RevisionTodo> {
        let snapshot = self.snapshot.as_ref()?;
        GroupedTodos::new(&snapshot.todos).todo_at_flat_index(self.selected_index)
    }

    fn grouped_todos(&self) -> GroupedTodos<'_> {
        GroupedTodos::new(&self.snapshot().todos)
    }

    fn snapshot(&self) -> &SessionSnapshot {
        self.snapshot.as_ref().expect("snapshot loaded")
    }

    fn is_read_only(&self) -> bool {
        matches!(self.snapshot().mode, RevisionMode::Historical(_))
    }

    fn is_read_only_snapshot(&self, snapshot: &SessionSnapshot) -> bool {
        matches!(snapshot.mode, RevisionMode::Historical(_))
    }

    fn active_footer_height(&self) -> Option<u16> {
        self.active_run.as_ref().map(|_| active_footer_height())
    }

    fn reselect(&mut self, todo_id: Option<i64>) {
        let (selected_index, total_len) = {
            let grouped = self.grouped_todos();
            let selected_index =
                if let Some(index) = todo_id.and_then(|todo_id| grouped.flat_index_of(todo_id)) {
                    index
                } else {
                    min(self.selected_index, grouped.len().saturating_sub(1))
                };
            (selected_index, grouped.len())
        };
        self.selected_index = selected_index;

        if total_len == 0 {
            self.selected_index = 0;
            self.open_scroll_offset = 0;
            self.completed_scroll_offset = 0;
            return;
        }

        self.ensure_selection_visible();
    }

    fn move_selection(&mut self, delta: isize) {
        let todo_count = self.grouped_todos().len();
        if todo_count == 0 {
            self.selected_index = 0;
            self.open_scroll_offset = 0;
            self.completed_scroll_offset = 0;
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
        self.ensure_selection_visible();
    }

    fn page_size(&self) -> usize {
        let grouped = self.grouped_todos();
        let areas = split_todo_list_area(self.layout_for_area(self.viewport_area).list);
        match grouped.section_row_for_flat_index(self.selected_index) {
            Some((TodoSection::Completed, _)) => section_visible_rows(areas.completed),
            _ => section_visible_rows(areas.open),
        }
    }

    fn ensure_selection_visible(&mut self) {
        let areas = split_todo_list_area(self.layout_for_area(self.viewport_area).list);
        let open_rows = section_visible_rows(areas.open);
        let completed_rows = section_visible_rows(areas.completed);
        let (open_len, completed_len, selected_section_row) = {
            let grouped = self.grouped_todos();
            (
                grouped.open().len(),
                grouped.completed().len(),
                grouped.section_row_for_flat_index(self.selected_index),
            )
        };

        self.open_scroll_offset = clamp_scroll_offset(self.open_scroll_offset, open_len, open_rows);
        self.completed_scroll_offset =
            clamp_scroll_offset(self.completed_scroll_offset, completed_len, completed_rows);

        let Some((section, row)) = selected_section_row else {
            return;
        };

        let (scroll_offset, visible_rows) = match section {
            TodoSection::Open => (&mut self.open_scroll_offset, open_rows),
            TodoSection::Completed => (&mut self.completed_scroll_offset, completed_rows),
        };
        if row < *scroll_offset {
            *scroll_offset = row;
        } else if row >= *scroll_offset + visible_rows {
            *scroll_offset = row + 1 - visible_rows;
        }
    }

    fn open_list_state(&self) -> ratatui::widgets::TableState {
        let grouped = self.grouped_todos();
        let selected_row = grouped
            .section_row_for_flat_index(self.selected_index)
            .and_then(|(section, row)| match section {
                TodoSection::Open => Some(row.saturating_sub(self.open_scroll_offset)),
                TodoSection::Completed => None,
            });
        section_state(selected_row)
    }

    fn completed_list_state(&self) -> ratatui::widgets::TableState {
        let grouped = self.grouped_todos();
        let selected_row = grouped
            .section_row_for_flat_index(self.selected_index)
            .and_then(|(section, row)| match section {
                TodoSection::Completed => Some(row.saturating_sub(self.completed_scroll_offset)),
                TodoSection::Open => None,
            });
        section_state(selected_row)
    }

    fn history_state(&self) -> ListState {
        let mut state = ListState::default();
        state.select(Some(self.history_index));
        state
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

    fn take_pending_bell(&mut self) -> bool {
        let pending = self.pending_completion_bell;
        self.pending_completion_bell = false;
        pending
    }

    fn open_todo_editor(&mut self) {
        if self.is_read_only() {
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
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
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
            return;
        }

        let Some((todo_id, title, notes, repo)) = self.current_todo().map(|todo| {
            (
                todo.todo_id,
                todo.title.clone(),
                todo.notes.clone(),
                todo.repo.clone().unwrap_or_default(),
            )
        }) else {
            return;
        };
        self.todo_editor = TodoEditorState {
            mode: TodoEditorMode::Edit { todo_id },
            title,
            notes,
            repo,
            focused_field: EditorField::Primary,
            error: None,
        };
        self.overlay = Some(Overlay::TodoEditor);
    }

    fn open_delete_todo(&mut self) {
        if self.is_read_only() {
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
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
            self.set_toast(
                String::from("Historical revisions are read-only"),
                ToastTone::Warning,
            );
            return;
        }
        let session = &self.snapshot().session;
        self.overlay = Some(Overlay::DeleteSession {
            name: session.name.clone(),
        });
    }

    fn open_details(&mut self) {
        self.overlay = Some(Overlay::Details);
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
                &self.session_name,
                title,
                self.todo_editor.notes.trim(),
                Some(self.todo_editor.repo.as_str()),
                now_utc_timestamp(),
            ),
            TodoEditorMode::Edit { todo_id } => database.update_todo(
                todo_id,
                Some(&self.session_name),
                title,
                self.todo_editor.notes.trim(),
                Some(self.todo_editor.repo.as_str()),
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
        self.set_toast(toast, ToastTone::Success);
        Ok(())
    }

    fn focused_todo_field(&mut self) -> &mut String {
        match self.todo_editor.focused_field {
            EditorField::Primary => &mut self.todo_editor.title,
            EditorField::Secondary => &mut self.todo_editor.notes,
            EditorField::Tertiary => &mut self.todo_editor.repo,
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

    fn current_todo_repo_details(&self, todo: &RevisionTodo) -> (Option<String>, &'static str) {
        if let Some(repo) = todo.repo.as_ref() {
            (Some(repo.clone()), "todo")
        } else if let Some(repo) = self.snapshot().session.repo.as_ref() {
            (Some(repo.clone()), "session")
        } else {
            (None, "-")
        }
    }
}

fn pomodoro_seconds(config: &Config, kind: PomodoroKind) -> i64 {
    match kind {
        PomodoroKind::Focus => i64::from(config.pomodoro.focus_minutes) * 60,
        PomodoroKind::ShortBreak => i64::from(config.pomodoro.short_break_minutes) * 60,
        PomodoroKind::LongBreak => i64::from(config.pomodoro.long_break_minutes) * 60,
    }
}

fn clamp_scroll_offset(offset: usize, total_rows: usize, visible_rows: usize) -> usize {
    total_rows.saturating_sub(visible_rows).min(offset)
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

    use super::{Overlay, SessionExit, SessionScreen, key_matches_binding};
    use crate::config::Config;
    use crate::db::Database;
    use crate::domain::pomodoro::{PomodoroKind, PomodoroState};
    use crate::domain::revision::RevisionMode;
    use crate::domain::todo::TodoStatus;
    use crate::tui::widgets::todo_list::{
        TodoClickTarget, TodoSection, split_todo_list_area, todo_click_target, todo_status_label,
        todo_time_label,
    };

    #[test]
    fn identifies_checkbox_and_row_click_targets() {
        let area = split_todo_list_area(Rect::new(0, 0, 40, 10));
        assert_eq!(todo_click_target(area, 0, 0, 0, 2), None);
        assert_eq!(
            todo_click_target(area, 0, 0, 1, 2),
            Some(TodoClickTarget::Checkbox {
                section: TodoSection::Open,
                row: 0,
            })
        );
        assert_eq!(
            todo_click_target(area, 0, 0, 6, 7),
            Some(TodoClickTarget::Row {
                section: TodoSection::Completed,
                row: 0,
            })
        );
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
    fn screen_handles_navigation_toggle_history_and_read_only_paths() {
        let (_directory, mut database, mut screen) = seeded_screen();
        assert!(
            screen
                .handle_key(&mut database, key(KeyCode::Char('h')))
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
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for character in "@SakanaAI/todui-keymove".chars() {
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
        assert_eq!(
            screen.current_todo().expect("selected").repo.as_deref(),
            Some("sakanaai/todui-keymove")
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
        screen.handle_key(&mut database, key(KeyCode::Tab)).unwrap();
        for character in "https://github.com/RobLange3/todui".chars() {
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
            screen.snapshot().todos[0].repo.as_deref(),
            Some("roblange3/todui")
        );

        assert_eq!(
            screen
                .handle_key(&mut database, key(KeyCode::Char('o')))
                .unwrap(),
            Some(SessionExit::Overview)
        );
    }

    #[test]
    fn screen_moves_selection_between_open_and_completed_sections() {
        let (_directory, mut database, mut screen) = seeded_screen();

        screen
            .handle_key(&mut database, key(KeyCode::Char('x')))
            .unwrap();
        screen
            .handle_key(&mut database, key(KeyCode::Home))
            .unwrap();
        assert_eq!(
            screen.current_todo().expect("selected").title,
            "Review bindings"
        );

        screen
            .handle_key(&mut database, key(KeyCode::Down))
            .unwrap();
        assert_eq!(screen.current_todo().expect("selected").title, "Draft spec");

        screen
            .handle_key(&mut database, key(KeyCode::Char('x')))
            .unwrap();
        assert_eq!(screen.current_todo().expect("selected").title, "Draft spec");

        screen
            .handle_key(&mut database, key(KeyCode::Home))
            .unwrap();
        assert_eq!(screen.current_todo().expect("selected").title, "Draft spec");

        screen.handle_key(&mut database, key(KeyCode::End)).unwrap();
        assert_eq!(
            screen.current_todo().expect("selected").title,
            "Review bindings"
        );
    }

    #[test]
    fn screen_scrolls_open_and_completed_sections_independently() {
        let (_directory, mut database, mut screen) = seeded_screen();
        let mut now = 1_711_275_900;
        let mut completed_ids = Vec::new();

        for title in ["Open 1", "Open 2", "Open 3", "Done 1", "Done 2", "Done 3"] {
            let todo = database
                .add_todo(&screen.session_name, title, "", None, now)
                .expect("todo");
            if title.starts_with("Done") {
                now += 10;
                database
                    .set_todo_status(todo.id, Some(&screen.session_name), TodoStatus::Done, now)
                    .expect("done");
                completed_ids.push(todo.id);
            }
            now += 10;
        }

        screen.reload(&database).expect("reload");
        let area = Rect::new(0, 0, 120, 10);

        screen
            .handle_key_in_area(&mut database, key(KeyCode::Home), area)
            .unwrap();

        let open_count = screen.grouped_todos().open().len();
        for _ in 0..open_count.saturating_sub(1) {
            screen
                .handle_key_in_area(&mut database, key(KeyCode::Down), area)
                .unwrap();
        }

        assert!(screen.open_scroll_offset > 0);
        assert_eq!(screen.completed_scroll_offset, 0);

        let open_scroll_offset = screen.open_scroll_offset;
        screen
            .handle_key_in_area(&mut database, key(KeyCode::Down), area)
            .unwrap();
        screen
            .handle_key_in_area(&mut database, key(KeyCode::Down), area)
            .unwrap();

        assert_eq!(screen.open_scroll_offset, open_scroll_offset);
        assert!(screen.completed_scroll_offset > 0);
        assert_eq!(
            screen.current_todo().expect("selected").todo_id,
            completed_ids[1]
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
    fn session_details_keys_open_close_and_fall_back_to_overview() {
        let (_directory, mut database, mut screen) = seeded_screen();

        assert!(screen.overlay.is_none());
        screen
            .handle_key(&mut database, key(KeyCode::Char('i')))
            .unwrap();
        assert!(matches!(screen.overlay, Some(Overlay::Details)));

        assert!(
            screen
                .handle_key(&mut database, key(KeyCode::Left))
                .unwrap()
                .is_none()
        );
        assert!(screen.overlay.is_none());

        screen
            .handle_key(&mut database, key(KeyCode::Right))
            .unwrap();
        assert!(matches!(screen.overlay, Some(Overlay::Details)));

        assert!(
            screen
                .handle_key(&mut database, key(KeyCode::Left))
                .unwrap()
                .is_none()
        );
        assert!(screen.overlay.is_none());

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
    fn read_only_header_keeps_help_hint() {
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

        let rendered = render_buffer(&screen, 120, 24);
        assert!(matches!(
            screen.snapshot().mode,
            RevisionMode::Historical(_)
        ));
        assert!(rendered.contains("read-only"));
        assert!(rendered.contains("h = help"));
        assert!(!rendered.contains("Keys"));
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
        assert!(screen.active_run.is_some());
        assert_eq!(screen.active_run.as_ref().unwrap().todo_id, None);

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().unwrap().state,
            PomodoroState::Paused
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('p')))
            .unwrap();
        assert!(matches!(
            screen.active_run.as_ref().unwrap().state,
            PomodoroState::Running
        ));

        screen
            .handle_key(&mut database, key(KeyCode::Char('c')))
            .unwrap();
        assert!(screen.active_run.is_none());

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
        assert!(database.get_session_by_name("writing-sprint").is_err());
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
            .start_pomodoro(PomodoroKind::Focus, 1, 0)
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
            .start_pomodoro(PomodoroKind::ShortBreak, 1, 0)
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
            .add_todo(
                &screen.session_name,
                "Ship live refresh",
                "",
                None,
                1_711_275_900,
            )
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
                &screen.session_name,
                "Should stay hidden",
                "",
                None,
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

        assert_eq!(screen.session_name, "writing-sprint");
        assert_eq!(screen.snapshot().todos.len(), 2);
    }

    #[test]
    fn render_covers_wide_medium_narrow_and_overlay_states() {
        let (_directory, _database, mut screen) = seeded_screen();

        let wide = render_buffer(&screen, 120, 24);
        assert!(wide.contains("writing-sprint"));
        assert!(wide.contains("Open"));
        assert!(wide.contains("Completed"));
        assert!(!wide.contains("Details"));
        let wide_lines = wide.lines().collect::<Vec<_>>();
        assert!(wide_lines[2].starts_with("└"));
        assert!(wide_lines[3].contains("Open"));
        assert!(wide.contains("h = help"));
        assert!(!wide.contains("Keys"));

        let medium = render_buffer(&screen, 80, 24);
        assert!(medium.contains("h = help"));
        assert!(!medium.contains("Details"));
        assert!(!medium.contains("Keys"));

        screen.overlay = Some(Overlay::Details);
        let details = render_buffer(&screen, 120, 24);
        assert!(details.contains("Details"));

        screen.overlay = Some(Overlay::History);
        let history = render_buffer(&screen, 120, 24);
        assert!(history.contains("History"));

        screen.overlay = Some(Overlay::Help);
        let help = render_buffer(&screen, 120, 24);
        assert!(help.contains("Navigation"));
        assert!(help.contains("Overview: Left or o"));
        assert!(help.contains("Details: i or Right"));
        assert!(help.contains("Help: h"));
        assert!(help.contains("Cancel timer: c"));

        screen.overlay = Some(Overlay::TodoEditor);
        let editor = render_buffer(&screen, 120, 24);
        assert!(editor.contains("New Todo"));
        assert!(editor.contains("Title"));

        screen.toast = Some(super::ToastState {
            message: String::from("hello"),
            expires_at: Instant::now() + Duration::from_secs(1),
            tone: super::ToastTone::Warning,
        });
        let toast = render_buffer(&screen, 120, 24);
        assert!(toast.contains("Notice"));
    }

    #[test]
    fn stacked_layout_hides_inline_details_and_keeps_pomodoro_at_sixty_columns() {
        let (_directory, _database, screen) = seeded_screen();

        let stacked = render_buffer(&screen, 60, 24);
        assert!(!stacked.contains("Details"));
        assert!(!stacked.contains("Pomodoro"));
    }

    #[test]
    fn tiny_layout_keeps_overlay_details_flow_below_fifty_columns() {
        let (_directory, _database, mut screen) = seeded_screen();

        let tiny = render_buffer(&screen, 49, 24);
        assert!(!tiny.contains("Details"));
        assert!(!tiny.contains("Pomodoro"));

        screen.overlay = Some(Overlay::Details);
        let details = render_buffer(&screen, 49, 24);
        assert!(details.contains("Details"));
    }

    #[test]
    fn active_footer_renders_in_narrow_session_layout() {
        let (_directory, mut database, mut screen) = seeded_screen();
        database
            .start_pomodoro(PomodoroKind::Focus, 1_500, 1_711_275_900)
            .expect("run");
        screen.reload(&database).expect("reload");

        let tiny = render_buffer(&screen, 49, 24);
        assert!(tiny.contains("Pomodoro"));
        assert!(tiny.contains("FOCUS"));
        assert!(!tiny.contains("Linked:"));
    }

    #[test]
    fn details_shortcuts_open_overlay_without_viewport_guard() {
        let (_directory, mut database, mut screen) = seeded_screen();

        screen
            .handle_key_in_area(&mut database, key(KeyCode::Right), Rect::new(0, 0, 60, 24))
            .unwrap();
        assert!(matches!(screen.overlay, Some(Overlay::Details)));

        screen.overlay = None;
        screen
            .handle_key_in_area(
                &mut database,
                key(KeyCode::Char('i')),
                Rect::new(0, 0, 49, 24),
            )
            .unwrap();
        assert!(matches!(screen.overlay, Some(Overlay::Details)));
    }

    #[test]
    fn idle_screen_hides_pomodoro_footer() {
        let (_directory, _database, screen) = seeded_screen();
        let wide = render_buffer(&screen, 120, 24);
        let medium = render_buffer(&screen, 80, 24);
        assert!(!wide.contains("Pomodoro"));
        assert!(!medium.contains("Pomodoro"));
    }

    #[test]
    fn active_focus_footer_renders_without_session_link() {
        let (_directory, mut database, mut screen) = seeded_screen();
        let run = database
            .start_pomodoro(PomodoroKind::Focus, 1_500, 1_711_275_900)
            .expect("run");
        screen.reload(&database).expect("reload");

        let rendered = render_buffer(&screen, 120, 24);
        assert!(rendered.contains("Pomodoro"));
        assert!(rendered.contains("FOCUS"));
        assert!(!rendered.contains("Linked:"));
        assert!(!rendered.contains("No linked todo"));
        assert_eq!(
            screen.active_run.as_ref().map(|active| active.id),
            Some(run.id)
        );
    }

    #[test]
    fn active_break_footer_renders_in_session_view() {
        let (_directory, mut database, mut screen) = seeded_screen();
        database
            .start_pomodoro(PomodoroKind::ShortBreak, 300, 1_711_275_900)
            .expect("run");
        screen.reload(&database).expect("reload");

        let rendered = render_buffer(&screen, 120, 24);
        assert!(rendered.contains("Pomodoro"));
        assert!(rendered.contains("SHORT BREAK"));
    }

    #[test]
    fn historical_revision_hides_pomodoro_ui() {
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

        let rendered = render_buffer(&screen, 120, 24);
        assert!(!rendered.contains("Pomodoro"));
        assert!(!rendered.contains("Focus runs up to this revision"));
    }

    #[test]
    fn help_overlay_lists_pause_and_cancel_controls() {
        let (_directory, _database, mut screen) = seeded_screen();

        screen.overlay = Some(Overlay::Help);

        let rendered = render_buffer(&screen, 120, 24);
        assert!(rendered.contains("Pomodoro: p start/pause/resume focus"));
        assert!(rendered.contains("Cancel timer: c"));
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
            .add_todo(&session.name, "Draft spec", "cover db", None, 1_711_275_700)
            .expect("todo");
        database
            .add_todo(&session.name, "Review bindings", "", None, 1_711_275_800)
            .expect("todo");

        let mut screen = SessionScreen::new(session.name, None, Config::default());
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
