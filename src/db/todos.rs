use rusqlite::{OptionalExtension, params};

use crate::db::Database;
use crate::db::sessions::create_revision_snapshot;
use crate::domain::todo::{Todo, TodoStatus};
use crate::error::{AppError, Result};

impl Database {
    pub fn add_todo(
        &mut self,
        session_slug: &str,
        title: &str,
        notes: &str,
        now: i64,
    ) -> Result<Todo> {
        let transaction = self.connection.transaction()?;
        let session_id = transaction
            .query_row(
                "SELECT id FROM sessions WHERE slug = ?1",
                [session_slug],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(session_slug.to_string()))?;

        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM todos WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;

        transaction.execute(
            "INSERT INTO todos (
                session_id, title, notes, status, position, created_at, updated_at, completed_at
             ) VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?5, NULL)",
            params![session_id, title, notes, position, now],
        )?;
        let todo_id = transaction.last_insert_rowid();

        transaction.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        let revision = create_revision_snapshot(&transaction, session_id, "todo added", now)?;
        transaction.execute(
            "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
            params![revision.revision_number, session_id],
        )?;
        transaction.commit()?;

        self.get_todo(todo_id)
    }

    pub fn set_todo_status(
        &mut self,
        todo_id: i64,
        session_slug: Option<&str>,
        status: TodoStatus,
        now: i64,
    ) -> Result<Todo> {
        let transaction = self.connection.transaction()?;
        let (session_id, current_status): (i64, String) = transaction
            .query_row(
                "SELECT session_id, status FROM todos WHERE id = ?1",
                [todo_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or(AppError::TodoNotFound(todo_id))?;

        if let Some(expected_session_slug) = session_slug {
            let actual_slug: String = transaction.query_row(
                "SELECT slug FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )?;
            if actual_slug != expected_session_slug {
                return Err(AppError::TodoSessionMismatch {
                    todo_id,
                    session: expected_session_slug.to_string(),
                });
            }
        }

        let next_status = status.as_str();
        if current_status != next_status {
            let completed_at = match status {
                TodoStatus::Open => None,
                TodoStatus::Done => Some(now),
            };
            transaction.execute(
                "UPDATE todos
                 SET status = ?1, completed_at = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![next_status, completed_at, now, todo_id],
            )?;
            transaction.execute(
                "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
                params![now, session_id],
            )?;
            let revision =
                create_revision_snapshot(&transaction, session_id, "todo status changed", now)?;
            transaction.execute(
                "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
                params![revision.revision_number, session_id],
            )?;
        }

        transaction.commit()?;
        self.get_todo(todo_id)
    }

    pub fn update_todo(
        &mut self,
        todo_id: i64,
        session_slug: Option<&str>,
        title: &str,
        notes: &str,
        now: i64,
    ) -> Result<Todo> {
        let transaction = self.connection.transaction()?;
        let (session_id, current_title, current_notes): (i64, String, String) = transaction
            .query_row(
                "SELECT session_id, title, notes FROM todos WHERE id = ?1",
                [todo_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(AppError::TodoNotFound(todo_id))?;

        if let Some(expected_session_slug) = session_slug {
            let actual_slug: String = transaction.query_row(
                "SELECT slug FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )?;
            if actual_slug != expected_session_slug {
                return Err(AppError::TodoSessionMismatch {
                    todo_id,
                    session: expected_session_slug.to_string(),
                });
            }
        }

        if current_title != title || current_notes != notes {
            transaction.execute(
                "UPDATE todos
                 SET title = ?1, notes = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![title, notes, now, todo_id],
            )?;
            transaction.execute(
                "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
                params![now, session_id],
            )?;
            let revision = create_revision_snapshot(&transaction, session_id, "todo edited", now)?;
            transaction.execute(
                "UPDATE sessions SET current_revision = ?1 WHERE id = ?2",
                params![revision.revision_number, session_id],
            )?;
        }

        transaction.commit()?;
        self.get_todo(todo_id)
    }

    pub fn get_live_todos(&self, session_id: i64) -> Result<Vec<Todo>> {
        let mut statement = self.connection.prepare(
            "SELECT id, session_id, title, notes, status, position, created_at, updated_at, completed_at
             FROM todos
             WHERE session_id = ?1
             ORDER BY position ASC",
        )?;
        let rows = statement.query_map([session_id], map_todo)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_todo(&self, todo_id: i64) -> Result<Todo> {
        self.connection
            .query_row(
                "SELECT id, session_id, title, notes, status, position, created_at, updated_at, completed_at
                 FROM todos WHERE id = ?1",
                [todo_id],
                map_todo,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::TodoNotFound(todo_id),
                other => AppError::Database(other),
            })
    }
}

fn map_todo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Todo> {
    let status = match row.get::<_, String>(4)?.as_str() {
        "done" => TodoStatus::Done,
        _ => TodoStatus::Open,
    };

    Ok(Todo {
        id: row.get(0)?,
        session_id: row.get(1)?,
        title: row.get(2)?,
        notes: row.get(3)?,
        status,
        position: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        completed_at: row.get(8)?,
    })
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::domain::todo::TodoStatus;

    #[test]
    fn adds_todos_and_tracks_revisions() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");
        let todo = database
            .add_todo(&session.slug, "Draft spec", "cover db", 1_711_275_700)
            .expect("todo");

        assert_eq!(todo.position, 1);

        let updated_session = database
            .get_session_by_slug(&session.slug)
            .expect("session");
        assert_eq!(updated_session.current_revision, 2);
    }

    #[test]
    fn toggles_done_and_undone_timestamps() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");
        let todo = database
            .add_todo(&session.slug, "Draft spec", "", 1_711_275_700)
            .expect("todo");

        let done = database
            .set_todo_status(
                todo.id,
                Some(&session.slug),
                TodoStatus::Done,
                1_711_275_800,
            )
            .expect("done");
        assert_eq!(done.status, TodoStatus::Done);
        assert_eq!(done.completed_at, Some(1_711_275_800));

        let reopened = database
            .set_todo_status(
                todo.id,
                Some(&session.slug),
                TodoStatus::Open,
                1_711_275_900,
            )
            .expect("open");
        assert_eq!(reopened.status, TodoStatus::Open);
        assert_eq!(reopened.completed_at, None);
    }

    #[test]
    fn edits_todo_title_and_notes_and_tracks_revision() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");
        let todo = database
            .add_todo(&session.slug, "Draft spec", "cover db", 1_711_275_700)
            .expect("todo");

        let edited = database
            .update_todo(
                todo.id,
                Some(&session.slug),
                "Draft final spec",
                "",
                1_711_275_800,
            )
            .expect("edited");

        assert_eq!(edited.title, "Draft final spec");
        assert_eq!(edited.notes, "");
        assert_eq!(edited.updated_at, 1_711_275_800);
        assert_eq!(
            database
                .get_session_by_slug(&session.slug)
                .expect("session")
                .current_revision,
            3
        );
    }
}
