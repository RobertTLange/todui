use rusqlite::{OptionalExtension, params};

use crate::db::Database;
use crate::domain::pomodoro::{PomodoroKind, PomodoroRun, PomodoroState, PomodoroSummary};
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

    pub fn start_pomodoro(
        &mut self,
        session_slug: &str,
        todo_id: Option<i64>,
        kind: PomodoroKind,
        planned_seconds: i64,
        now: i64,
    ) -> Result<PomodoroRun> {
        let transaction = self.connection.transaction()?;
        let session_id: i64 = transaction
            .query_row(
                "SELECT id FROM sessions WHERE slug = ?1",
                [session_slug],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(session_slug.to_string()))?;

        if let Some(todo_id) = todo_id {
            let todo_session_id: i64 = transaction
                .query_row(
                    "SELECT session_id FROM todos WHERE id = ?1",
                    [todo_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(AppError::TodoNotFound(todo_id))?;
            if todo_session_id != session_id {
                return Err(AppError::TodoSessionMismatch {
                    todo_id,
                    session: session_slug.to_string(),
                });
            }
        }

        let insert = transaction.execute(
            "INSERT INTO pomodoro_runs (
                session_id, todo_id, kind, state, planned_seconds, started_at, paused_at, accumulated_pause, ended_at, updated_at
             ) VALUES (?1, ?2, ?3, 'running', ?4, ?5, NULL, 0, NULL, ?5)",
            params![session_id, todo_id, kind.as_str(), planned_seconds, now],
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

    pub fn pomodoro_summary_for_session(
        &self,
        session_id: i64,
        up_to: Option<i64>,
    ) -> Result<PomodoroSummary> {
        let query = match up_to {
            Some(_) => {
                "SELECT COUNT(*),
                        COALESCE(SUM(ended_at - started_at - accumulated_pause), 0)
                 FROM pomodoro_runs
                 WHERE session_id = ?1
                   AND kind = 'focus'
                   AND state = 'completed'
                   AND ended_at IS NOT NULL
                   AND ended_at <= ?2"
            }
            None => {
                "SELECT COUNT(*),
                        COALESCE(SUM(ended_at - started_at - accumulated_pause), 0)
                 FROM pomodoro_runs
                 WHERE session_id = ?1
                   AND kind = 'focus'
                   AND state = 'completed'
                   AND ended_at IS NOT NULL"
            }
        };

        let summary = if let Some(upper_bound) = up_to {
            self.connection
                .query_row(query, params![session_id, upper_bound], |row| {
                    Ok(PomodoroSummary {
                        completed_focus_runs: row.get(0)?,
                        total_focus_seconds: row.get(1)?,
                    })
                })?
        } else {
            self.connection.query_row(query, [session_id], |row| {
                Ok(PomodoroSummary {
                    completed_focus_runs: row.get(0)?,
                    total_focus_seconds: row.get(1)?,
                })
            })?
        };

        Ok(summary)
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
    use crate::domain::pomodoro::PomodoroKind;

    #[test]
    fn enforces_single_active_pomodoro() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, None, 1_711_275_600)
            .expect("session");

        let run = database
            .start_pomodoro(
                &session.slug,
                None,
                PomodoroKind::Focus,
                1_500,
                1_711_275_700,
            )
            .expect("run");
        let second = database.start_pomodoro(
            &session.slug,
            None,
            PomodoroKind::ShortBreak,
            300,
            1_711_275_701,
        );
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
        assert_eq!(
            completed.state,
            crate::domain::pomodoro::PomodoroState::Completed
        );

        let summary = database
            .pomodoro_summary_for_session(session.id, None)
            .expect("summary");
        assert_eq!(summary.completed_focus_runs, 1);
    }
}
