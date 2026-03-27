use rusqlite::{OptionalExtension, params};

use crate::db::Database;
use crate::domain::pomodoro::{PomodoroKind, PomodoroRun, PomodoroState};
use crate::error::{AppError, Result};

impl Database {
    pub fn get_active_pomodoro(&self) -> Result<Option<PomodoroRun>> {
        self.connection
            .query_row(
                "SELECT id, session_id, todo_id, kind, state, planned_seconds, started_at, paused_at, accumulated_pause, ended_at, updated_at
                 FROM pomodoro_runs
                 WHERE state IN ('running', 'paused')
                 LIMIT 1",
                [],
                map_pomodoro_run,
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn get_active_pomodoro_with_link(&self) -> Result<Option<(PomodoroRun, Option<String>)>> {
        self.connection
            .query_row(
                "SELECT runs.id, runs.session_id, runs.todo_id, runs.kind, runs.state, runs.planned_seconds, runs.started_at, runs.paused_at, runs.accumulated_pause, runs.ended_at, runs.updated_at,
                        todos.title
                 FROM pomodoro_runs runs
                 LEFT JOIN todos ON todos.id = runs.todo_id
                 WHERE runs.state IN ('running', 'paused')
                 LIMIT 1",
                [],
                |row| Ok((map_pomodoro_run(row)?, row.get(11)?)),
            )
            .optional()
            .map_err(AppError::from)
    }

    pub fn start_pomodoro(
        &mut self,
        todo_id: Option<i64>,
        kind: PomodoroKind,
        planned_seconds: i64,
        now: i64,
    ) -> Result<PomodoroRun> {
        let transaction = self.connection.transaction()?;
        if let Some(todo_id) = todo_id {
            transaction
                .query_row("SELECT id FROM todos WHERE id = ?1", [todo_id], |row| {
                    row.get::<_, i64>(0)
                })
                .optional()?
                .ok_or(AppError::TodoNotFound(todo_id))?;
        }

        let insert = transaction.execute(
            "INSERT INTO pomodoro_runs (
                session_id, todo_id, kind, state, planned_seconds, started_at, paused_at, accumulated_pause, ended_at, updated_at
             ) VALUES (?1, ?2, ?3, 'running', ?4, ?5, NULL, 0, NULL, ?5)",
            params![Option::<i64>::None, todo_id, kind.as_str(), planned_seconds, now],
        );
        match insert {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                return Err(AppError::ActivePomodoroExists);
            }
            Err(other) => return Err(AppError::Database(other)),
        }
        let run_id = transaction.last_insert_rowid();
        transaction.commit()?;

        self.get_pomodoro_run(run_id)
    }

    pub fn pause_pomodoro(&mut self, run_id: i64, now: i64) -> Result<PomodoroRun> {
        self.connection.execute(
            "UPDATE pomodoro_runs
             SET state = 'paused', paused_at = ?1, updated_at = ?1
             WHERE id = ?2 AND state = 'running'",
            params![now, run_id],
        )?;
        self.get_pomodoro_run(run_id)
    }

    pub fn resume_pomodoro(&mut self, run_id: i64, now: i64) -> Result<PomodoroRun> {
        let _paused_at: i64 = self
            .connection
            .query_row(
                "SELECT paused_at FROM pomodoro_runs WHERE id = ?1 AND state = 'paused'",
                [run_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(AppError::TodoNotFound(run_id))?;

        self.connection.execute(
            "UPDATE pomodoro_runs
             SET state = 'running',
                 accumulated_pause = accumulated_pause + (?1 - paused_at),
                 paused_at = NULL,
                 updated_at = ?1
             WHERE id = ?2",
            params![now, run_id],
        )?;
        self.get_pomodoro_run(run_id)
    }

    pub fn cancel_pomodoro(&mut self, run_id: i64, now: i64) -> Result<PomodoroRun> {
        self.connection.execute(
            "UPDATE pomodoro_runs
             SET state = 'cancelled', ended_at = ?1, updated_at = ?1
             WHERE id = ?2 AND state IN ('running', 'paused')",
            params![now, run_id],
        )?;
        self.get_pomodoro_run(run_id)
    }

    pub fn complete_pomodoro(&mut self, run_id: i64, now: i64) -> Result<PomodoroRun> {
        self.connection.execute(
            "UPDATE pomodoro_runs
             SET state = 'completed', ended_at = ?1, updated_at = ?1, paused_at = NULL
             WHERE id = ?2 AND state IN ('running', 'paused')",
            params![now, run_id],
        )?;
        self.get_pomodoro_run(run_id)
    }

    pub fn get_pomodoro_run(&self, run_id: i64) -> Result<PomodoroRun> {
        self.connection
            .query_row(
                "SELECT id, session_id, todo_id, kind, state, planned_seconds, started_at, paused_at, accumulated_pause, ended_at, updated_at
                 FROM pomodoro_runs
                 WHERE id = ?1",
                [run_id],
                map_pomodoro_run,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::TodoNotFound(run_id),
                other => AppError::Database(other),
            })
    }
}

fn map_pomodoro_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<PomodoroRun> {
    let kind = match row.get::<_, String>(3)?.as_str() {
        "short_break" => PomodoroKind::ShortBreak,
        "long_break" => PomodoroKind::LongBreak,
        _ => PomodoroKind::Focus,
    };
    let state = match row.get::<_, String>(4)?.as_str() {
        "paused" => PomodoroState::Paused,
        "completed" => PomodoroState::Completed,
        "cancelled" => PomodoroState::Cancelled,
        _ => PomodoroState::Running,
    };

    Ok(PomodoroRun {
        id: row.get(0)?,
        session_id: row.get(1)?,
        todo_id: row.get(2)?,
        kind,
        state,
        planned_seconds: row.get(5)?,
        started_at: row.get(6)?,
        paused_at: row.get(7)?,
        accumulated_pause: row.get(8)?,
        ended_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::domain::pomodoro::{PomodoroKind, PomodoroState};

    #[test]
    fn enforces_single_active_pomodoro() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");

        let run = database
            .start_pomodoro(None, PomodoroKind::Focus, 1_500, 1_711_275_700)
            .expect("run");
        let second = database.start_pomodoro(None, PomodoroKind::ShortBreak, 300, 1_711_275_701);
        assert!(second.is_err());

        let paused = database
            .pause_pomodoro(run.id, 1_711_275_800)
            .expect("paused");
        assert_eq!(paused.state, crate::domain::pomodoro::PomodoroState::Paused);
        let resumed = database
            .resume_pomodoro(run.id, 1_711_275_900)
            .expect("resumed");
        assert_eq!(resumed.accumulated_pause, 100);
        let completed = database
            .complete_pomodoro(run.id, 1_711_276_000)
            .expect("completed");
        assert_eq!(completed.state, PomodoroState::Completed);
    }

    #[test]
    fn starts_unlinked_global_pomodoro() {
        let (_directory, mut database) = Database::open_temp().expect("database");

        let run = database
            .start_pomodoro(None, PomodoroKind::Focus, 1_500, 1_711_275_700)
            .expect("run");

        assert_eq!(run.session_id, None);
        assert_eq!(run.todo_id, None);
        assert_eq!(run.state, PomodoroState::Running);
    }

    #[test]
    fn reads_active_pomodoro_with_optional_linked_title() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");
        let todo = database
            .add_todo(&session.slug, "Draft spec", "", 1_711_275_700)
            .expect("todo");
        let run = database
            .start_pomodoro(Some(todo.id), PomodoroKind::Focus, 1_500, 1_711_275_750)
            .expect("run");

        let active = database
            .get_active_pomodoro_with_link()
            .expect("active")
            .expect("run");

        assert_eq!(active.0.id, run.id);
        assert_eq!(active.1.as_deref(), Some("Draft spec"));
    }
}
