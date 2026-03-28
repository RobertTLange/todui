#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenRecentSession,
    OpenSession {
        name: String,
    },
    OpenRevision {
        name: String,
        revision: u32,
    },
    CloseOverlay,
    Quit,
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    GoTop,
    GoBottom,
    SelectTodo {
        id: i64,
    },
    NewTodo,
    EditTodo {
        id: i64,
    },
    SaveTodo {
        id: i64,
        title: String,
        notes: String,
    },
    AddTodo {
        session_name: String,
        title: String,
        notes: String,
    },
    ToggleTodo {
        id: i64,
    },
    OpenHistory,
    SelectRevision {
        revision: u32,
    },
    StartPomodoro {
        kind: crate::domain::pomodoro::PomodoroKind,
    },
    PausePomodoro,
    ResumePomodoro,
    CancelPomodoro,
    Tick,
    MouseClick {
        x: u16,
        y: u16,
    },
    MouseScrollUp,
    MouseScrollDown,
}
