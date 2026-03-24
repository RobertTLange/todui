use crate::domain::pomodoro::PomodoroRun;
use crate::domain::revision::RevisionMode;
use crate::domain::session::SessionView;
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub current_session: Option<SessionView>,
    pub current_revision_mode: RevisionMode,
    pub selected_todo_id: Option<i64>,
    pub focused_pane: FocusedPane,
    pub overlay: Option<Overlay>,
    pub toast: Option<Toast>,
    pub theme: Theme,
    pub active_pomodoro: Option<PomodoroRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusedPane {
    #[default]
    TodoList,
    Details,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Help,
    History,
    TodoEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub message: String,
}
