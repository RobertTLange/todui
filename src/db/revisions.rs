use rusqlite::{OptionalExtension, params};

use crate::db::Database;
use crate::db::sessions::map_revision_summary;
use crate::domain::revision::{RevisionMode, RevisionSummary, RevisionTodo, SessionSnapshot};
use crate::error::{AppError, Result};

impl Database {
    pub fn list_revisions(&self, session_slug: &str) -> Result<Vec<RevisionSummary>> {
        let session_id: i64 = self
            .connection
            .query_row(
                "SELECT id FROM sessions WHERE slug = ?1",
                [session_slug],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::SessionNotFound(session_slug.to_string()))?;

        let mut statement = self.connection.prepare(
            "SELECT revision_number, created_at, reason, todo_count, done_count
             FROM session_revisions
             WHERE session_id = ?1
             ORDER BY revision_number DESC",
        )?;
        let rows = statement.query_map([session_id], map_revision_summary)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn get_revision_todos(
        &self,
        session_slug: &str,
        revision_number: u32,
    ) -> Result<Vec<RevisionTodo>> {
        let revision_id: i64 = self
            .connection
            .query_row(
                "SELECT revisions.id
                 FROM session_revisions revisions
                 JOIN sessions ON sessions.id = revisions.session_id
                 WHERE sessions.slug = ?1 AND revisions.revision_number = ?2",
                params![session_slug, revision_number],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::RevisionNotFound {
                session: session_slug.to_string(),
                revision: revision_number,
            })?;

        let mut statement = self.connection.prepare(
            "SELECT todo_id, title, notes, status, position, created_at, updated_at, completed_at
             FROM session_revision_todos
             WHERE revision_id = ?1
             ORDER BY position ASC",
        )?;
        let rows = statement.query_map([revision_id], map_revision_todo)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn load_snapshot(
        &self,
        session_slug: &str,
        revision: Option<u32>,
    ) -> Result<SessionSnapshot> {
        let session = self.get_session_by_slug(session_slug)?;
        let revision_summary = match revision {
            Some(revision_number) => self.revision_summary(session_slug, revision_number)?,
            None => self.current_revision_summary(session.id)?,
        };
        let todos = match revision {
            Some(revision_number) => self.get_revision_todos(session_slug, revision_number)?,
            None => self
                .get_live_todos(session.id)?
                .into_iter()
                .map(|todo| RevisionTodo {
                    todo_id: todo.id,
                    title: todo.title,
                    notes: todo.notes,
                    status: todo.status,
                    position: todo.position,
                    created_at: todo.created_at,
                    updated_at: todo.updated_at,
                    completed_at: todo.completed_at,
                })
                .collect(),
        };

        Ok(SessionSnapshot {
            session,
            revision: revision_summary,
            todos,
            mode: revision.map_or(RevisionMode::Head, RevisionMode::Historical),
        })
    }

    pub fn revision_summary(
        &self,
        session_slug: &str,
        revision_number: u32,
    ) -> Result<RevisionSummary> {
        self.connection
            .query_row(
                "SELECT revisions.revision_number, revisions.created_at, revisions.reason, revisions.todo_count, revisions.done_count
                 FROM session_revisions revisions
                 JOIN sessions ON sessions.id = revisions.session_id
                 WHERE sessions.slug = ?1 AND revisions.revision_number = ?2",
                params![session_slug, revision_number],
                map_revision_summary,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => AppError::RevisionNotFound {
                    session: session_slug.to_string(),
                    revision: revision_number,
                },
                other => AppError::Database(other),
            })
    }
}

fn map_revision_todo(row: &rusqlite::Row<'_>) -> rusqlite::Result<RevisionTodo> {
    let status = match row.get::<_, String>(3)?.as_str() {
        "done" => crate::domain::todo::TodoStatus::Done,
        _ => crate::domain::todo::TodoStatus::Open,
    };

    Ok(RevisionTodo {
        todo_id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        status,
        position: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        completed_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::domain::todo::TodoStatus;
    use crate::error::AppError;

    #[test]
    fn lists_revision_history() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");
        let todo = database
            .add_todo(&session.slug, "Draft spec", "", 1_711_275_700)
            .expect("todo");
        database
            .set_todo_status(
                todo.id,
                Some(&session.slug),
                TodoStatus::Done,
                1_711_275_800,
            )
            .expect("done");

        let revisions = database.list_revisions(&session.slug).expect("revisions");
        assert_eq!(revisions.len(), 3);
        assert_eq!(revisions[0].revision_number, 3);
        assert_eq!(revisions[2].revision_number, 1);
    }

    #[test]
    fn loads_head_and_historical_snapshots() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");
        let todo = database
            .add_todo(&session.slug, "Draft spec", "note", 1_711_275_700)
            .expect("todo");
        database
            .set_todo_status(
                todo.id,
                Some(&session.slug),
                TodoStatus::Done,
                1_711_275_800,
            )
            .expect("done");

        let head = database.load_snapshot(&session.slug, None).expect("head");
        assert_eq!(head.todos.len(), 1);
        assert!(matches!(
            head.mode,
            crate::domain::revision::RevisionMode::Head
        ));

        let historical = database
            .load_snapshot(&session.slug, Some(1))
            .expect("revision");
        assert!(historical.todos.is_empty());
        assert!(matches!(
            historical.mode,
            crate::domain::revision::RevisionMode::Historical(1)
        ));
    }

    #[test]
    fn returns_revision_not_found_for_missing_revision() {
        let (_directory, mut database) = Database::open_temp().expect("database");
        let session = database
            .create_session("Writing Sprint", None, 1_711_275_600)
            .expect("session");

        let error = database
            .revision_summary(&session.slug, 9)
            .expect_err("missing revision");
        assert!(matches!(
            error,
            AppError::RevisionNotFound {
                session: _,
                revision: 9
            }
        ));
    }
}
