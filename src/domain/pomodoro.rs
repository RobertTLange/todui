#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomodoroKind {
    Focus,
    ShortBreak,
    LongBreak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PomodoroState {
    Running,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PomodoroRun {
    pub id: i64,
    pub session_id: Option<i64>,
    pub todo_id: Option<i64>,
    pub kind: PomodoroKind,
    pub state: PomodoroState,
    pub planned_seconds: i64,
    pub started_at: i64,
    pub paused_at: Option<i64>,
    pub accumulated_pause: i64,
    pub ended_at: Option<i64>,
    pub updated_at: i64,
}

impl PomodoroKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Focus => "FOCUS",
            Self::ShortBreak => "SHORT BREAK",
            Self::LongBreak => "LONG BREAK",
        }
    }
}

impl PomodoroState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

pub fn remaining_seconds(run: &PomodoroRun, now: i64) -> i64 {
    let elapsed = match run.state {
        PomodoroState::Running => now - run.started_at - run.accumulated_pause,
        PomodoroState::Paused => {
            run.paused_at.unwrap_or(now) - run.started_at - run.accumulated_pause
        }
        PomodoroState::Completed | PomodoroState::Cancelled => run.planned_seconds,
    };

    (run.planned_seconds - elapsed).max(0)
}

pub fn progress_ratio(run: &PomodoroRun, now: i64) -> f64 {
    if run.planned_seconds <= 0 {
        return 1.0;
    }

    let consumed = run.planned_seconds - remaining_seconds(run, now);
    (consumed as f64 / run.planned_seconds as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{PomodoroKind, PomodoroRun, PomodoroState, progress_ratio, remaining_seconds};

    #[test]
    fn computes_remaining_seconds_for_running_and_paused_runs() {
        let running = PomodoroRun {
            id: 1,
            session_id: None,
            todo_id: None,
            kind: PomodoroKind::Focus,
            state: PomodoroState::Running,
            planned_seconds: 1_500,
            started_at: 100,
            paused_at: None,
            accumulated_pause: 30,
            ended_at: None,
            updated_at: 100,
        };
        assert_eq!(remaining_seconds(&running, 400), 1_230);

        let paused = PomodoroRun {
            paused_at: Some(300),
            state: PomodoroState::Paused,
            ..running
        };
        assert_eq!(remaining_seconds(&paused, 500), 1_330);
        assert!(progress_ratio(&paused, 500) > 0.0);
    }
}
